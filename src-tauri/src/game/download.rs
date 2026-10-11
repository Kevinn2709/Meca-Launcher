//! Descargas: solo por HTTPS, solo de servidores de confianza y siempre
//! comprobando el tamaño y la huella de lo que llega.

use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::config::{
    ALLOWED_HOSTS, CONNECT_TIMEOUT, DOWNLOAD_ATTEMPTS, METADATA_LIMIT, PARALLEL_DOWNLOADS,
    READ_TIMEOUT, UNKNOWN_SIZE_LIMIT, USER_AGENT,
};
use super::error::GameError;
use super::events::DownloadProgress;
use super::hash::{self, Expected, Hasher};
use super::task;

/// Intentos para las listas e índices: son pequeños y, si no hay conexión,
/// conviene saberlo pronto para usar la copia guardada.
const METADATA_ATTEMPTS: u32 = 3;

/// Pausa antes de reintentar; crece con cada intento (1 s, 2 s, 3 s…).
const RETRY_PAUSE: Duration = Duration::from_secs(1);

/// Un archivo por descargar.
#[derive(Debug, Clone)]
pub struct Download {
    pub url: String,
    pub dest: PathBuf,
    /// Tamaño exacto en bytes, si se conoce.
    pub size: Option<u64>,
    pub hash: Option<Expected>,
    /// Nombre que ve el jugador mientras se descarga.
    pub label: String,
}

pub fn is_allowed_host(host: &str) -> bool {
    ALLOWED_HOSTS.contains(&host)
}

/// Servidor de una dirección con la forma exacta `https://servidor/ruta`.
/// Cualquier otra forma (otro protocolo, usuario, puerto, espacios) da `None`.
pub fn https_host(url: &str) -> Option<&str> {
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let (host, path) = url.strip_prefix("https://")?.split_once('/')?;
    let plain = !host.is_empty()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'-');
    (plain && !path.is_empty()).then_some(host)
}

/// Rechaza cualquier dirección que no sea HTTPS hacia un servidor de confianza.
pub fn check_url(url: &str) -> Result<(), GameError> {
    match https_host(url) {
        Some(host) if is_allowed_host(host) => Ok(()),
        _ => {
            log::warn!("descarga rechazada: {url}");
            Err(GameError::new(
                "blocked_url",
                "El launcher rechazó una descarga porque no viene de un servidor de confianza.",
            ))
        }
    }
}

/// Cliente HTTP para descargas. Sin límite de tiempo total (hay archivos
/// grandes), pero corta si la conexión se queda parada.
pub fn client() -> reqwest::Client {
    // Una redirección solo se sigue si lleva a otro servidor de confianza.
    let redirects = reqwest::redirect::Policy::custom(|attempt| {
        let trusted = attempt.previous().len() < 5
            && attempt.url().scheme() == "https"
            && attempt.url().host_str().is_some_and(is_allowed_host);
        if trusted {
            attempt.follow()
        } else {
            attempt.stop()
        }
    });

    // Si el cliente no se puede crear es que el sistema no tiene TLS: sin él
    // no hay launcher. No se sustituye por uno sin la regla de redirecciones.
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .redirect(redirects)
        .build()
        .expect("no se pudo preparar el cliente de descargas")
}

/// Qué hacer con un intento fallido.
enum Failure {
    /// Puede salir bien si se repite: corte de red, servidor saturado, archivo dañado.
    Retry(GameError),
    /// Repetir no lo arregla: el archivo no existe o el disco falla.
    Fatal(GameError),
}

fn network_failure(error: reqwest::Error) -> Failure {
    log::warn!("fallo de red: {error}");
    Failure::Retry(GameError::network())
}

fn disk_failure(context: &'static str) -> impl Fn(io::Error) -> Failure {
    move |error| Failure::Fatal(GameError::disk(context, &error))
}

fn check_status(code: u16, url: &str) -> Result<(), Failure> {
    if (200..300).contains(&code) {
        return Ok(());
    }
    log::warn!("{url} respondió {code}");
    let error = GameError::new(
        "unavailable",
        "Un servidor de descargas no entregó un archivo. Inténtalo de nuevo en unos minutos.",
    );
    Err(if code >= 500 || code == 429 || code == 408 {
        Failure::Retry(error)
    } else {
        Failure::Fatal(error)
    })
}

/// Descarga a memoria un archivo de datos pequeño (listas e índices).
pub async fn fetch(
    http: &reqwest::Client,
    url: &str,
    expected: Option<&Expected>,
) -> Result<Vec<u8>, GameError> {
    check_url(url)?;
    let mut last = GameError::network();
    for attempt in 1..=METADATA_ATTEMPTS {
        if attempt > 1 {
            task::pause(RETRY_PAUSE * (attempt - 1)).await;
        }
        match fetch_once(http, url, expected).await {
            Ok(data) => return Ok(data),
            Err(Failure::Fatal(error)) => return Err(error),
            Err(Failure::Retry(error)) => last = error,
        }
    }
    Err(last)
}

async fn fetch_once(
    http: &reqwest::Client,
    url: &str,
    expected: Option<&Expected>,
) -> Result<Vec<u8>, Failure> {
    let mut response = http.get(url).send().await.map_err(network_failure)?;
    check_status(response.status().as_u16(), url)?;

    let mut data = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(network_failure)? {
        if (data.len() + chunk.len()) as u64 > METADATA_LIMIT {
            log::warn!("{url} es más grande de lo razonable");
            return Err(Failure::Fatal(GameError::invalid("Un archivo de datos")));
        }
        data.extend_from_slice(&chunk);
    }

    if let Some(expected) = expected {
        if !hash::bytes_match(&data, expected) {
            log::warn!("{url} no coincide con su huella");
            return Err(Failure::Retry(GameError::corrupt()));
        }
    }
    Ok(data)
}

/// Descarga todos los archivos, varios a la vez. Se detiene en el primer
/// fallo que no se arregla reintentando.
pub async fn run(
    http: &reqwest::Client,
    mut jobs: Vec<Download>,
    progress: Option<Arc<DownloadProgress>>,
) -> Result<(), GameError> {
    // Antes de descargar nada se comprueba que todas las direcciones son de confianza.
    for job in &jobs {
        check_url(&job.url)?;
    }
    // Los archivos grandes primero: así no quedan solos al final.
    jobs.sort_by(|a, b| b.size.cmp(&a.size));

    let workers = PARALLEL_DOWNLOADS.min(jobs.len());
    let pool = Arc::new(Pool {
        queue: Mutex::new(VecDeque::from(jobs)),
        failure: Mutex::new(None),
        stopped: AtomicBool::new(false),
    });

    let handles: Vec<_> = (0..workers)
        .map(|_| tauri::async_runtime::spawn(worker(http.clone(), pool.clone(), progress.clone())))
        .collect();
    // Se espera a todas antes de dar el resultado: así ninguna descarga
    // sigue escribiendo por detrás si el jugador pulsa Reintentar.
    for handle in handles {
        if let Err(error) = handle.await {
            log::warn!("una descarga no terminó: {error}");
            pool.fail(GameError::internal());
        }
    }

    if let Some(progress) = &progress {
        progress.finish();
    }
    match pool.failure.lock().unwrap_or_else(PoisonError::into_inner).take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Lo que comparten las descargas simultáneas.
struct Pool {
    queue: Mutex<VecDeque<Download>>,
    /// Primer fallo que no se arregló reintentando.
    failure: Mutex<Option<GameError>>,
    /// `true` en cuanto una descarga falla: las demás se detienen enseguida.
    stopped: AtomicBool,
}

impl Pool {
    fn next(&self) -> Option<Download> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner).pop_front()
    }

    fn fail(&self, error: GameError) {
        self.stopped.store(true, Ordering::SeqCst);
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert(error);
    }
}

async fn worker(http: reqwest::Client, pool: Arc<Pool>, progress: Option<Arc<DownloadProgress>>) {
    while !pool.stopped.load(Ordering::SeqCst) {
        let Some(job) = pool.next() else {
            break;
        };
        if let Err(error) = download_one(&http, &job, progress.as_deref(), &pool.stopped).await {
            pool.fail(error);
            break;
        }
    }
}

/// `archivo.jar` -> `archivo.jar.part`. Se descarga ahí y solo al terminar
/// bien se le da el nombre definitivo: nunca queda un archivo a medias.
fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

async fn download_one(
    http: &reqwest::Client,
    job: &Download,
    progress: Option<&DownloadProgress>,
    stopped: &AtomicBool,
) -> Result<(), GameError> {
    if let Some(parent) = job.dest.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| GameError::disk("no se pudo crear una carpeta", &error))?;
    }
    let part = part_path(&job.dest);

    // Un archivo de tamaño desconocido se suma al total una sola vez,
    // aunque haga falta más de un intento.
    let mut announced = false;
    let mut last = GameError::network();
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        if attempt > 1 {
            task::pause(RETRY_PAUSE * (attempt - 1)).await;
        }
        if stopped.load(Ordering::SeqCst) {
            return Err(last);
        }
        let mut counted = 0;
        let tracking = Tracking {
            progress,
            stopped,
            counted: &mut counted,
            announced: &mut announced,
        };
        match transfer(http, job, &part, tracking).await {
            Ok(()) => return place(&part, &job.dest).await,
            Err(failure) => {
                if let Some(progress) = progress {
                    progress.undo(counted);
                }
                let _ = fs::remove_file(&part);
                match failure {
                    Failure::Fatal(error) => return Err(error),
                    Failure::Retry(error) => last = error,
                }
            }
        }
    }
    Err(last)
}

/// Da al archivo descargado su nombre definitivo. En Windows el antivirus
/// retiene un instante los archivos recién escritos, así que se insiste un poco.
async fn place(part: &Path, dest: &Path) -> Result<(), GameError> {
    const TRIES: u32 = 8;
    let mut attempt = 1;
    loop {
        match fs::rename(part, dest) {
            Ok(()) => return Ok(()),
            Err(error) if attempt < TRIES && error.kind() != io::ErrorKind::NotFound => {
                task::pause(Duration::from_millis(60) * attempt).await;
                attempt += 1;
            }
            Err(error) => return Err(GameError::disk("no se pudo colocar un archivo", &error)),
        }
    }
}

/// Lo que un intento de descarga necesita saber del resto.
struct Tracking<'a> {
    progress: Option<&'a DownloadProgress>,
    /// Otra descarga falló: no tiene sentido seguir con esta.
    stopped: &'a AtomicBool,
    /// Acumula lo que este intento sumó al progreso.
    counted: &'a mut u64,
    /// `true` cuando el tamaño de este archivo ya se sumó al total.
    announced: &'a mut bool,
}

/// Un intento: baja el archivo a `part` y comprueba tamaño y huella.
async fn transfer(
    http: &reqwest::Client,
    job: &Download,
    part: &Path,
    tracking: Tracking<'_>,
) -> Result<(), Failure> {
    let Tracking {
        progress,
        stopped,
        counted,
        announced,
    } = tracking;

    let mut response = http
        .get(job.url.as_str())
        .send()
        .await
        .map_err(network_failure)?;
    check_status(response.status().as_u16(), &job.url)?;

    let limit = job.size.unwrap_or(UNKNOWN_SIZE_LIMIT);
    if let (None, Some(progress), false) = (job.size, progress, *announced) {
        if let Some(length) = response.content_length().filter(|length| *length <= limit) {
            progress.expect(length);
            *announced = true;
        }
    }
    let mut file = File::create(part).map_err(disk_failure("no se pudo crear un archivo"))?;
    let mut hasher = job.hash.as_ref().map(Hasher::matching);
    let mut received: u64 = 0;

    while let Some(chunk) = response.chunk().await.map_err(network_failure)? {
        if stopped.load(Ordering::SeqCst) {
            return Err(Failure::Fatal(GameError::network()));
        }
        let length = chunk.len() as u64;
        received += length;
        if received > limit {
            log::warn!("{} es más grande de lo esperado", job.url);
            return Err(Failure::Retry(GameError::corrupt()));
        }
        file.write_all(&chunk)
            .map_err(disk_failure("no se pudo escribir un archivo"))?;
        if let Some(hasher) = hasher.as_mut() {
            hasher.update(&chunk);
        }
        if let Some(progress) = progress {
            progress.add(&job.label, length);
            *counted += length;
        }
    }
    file.flush()
        .map_err(disk_failure("no se pudo terminar de escribir un archivo"))?;
    drop(file);

    if job.size.is_some_and(|size| size != received) {
        log::warn!("{} llegó con {received} bytes", job.url);
        return Err(Failure::Retry(GameError::corrupt()));
    }
    if let (Some(hasher), Some(expected)) = (hasher, job.hash.as_ref()) {
        if !expected.matches(&hasher.finish()) {
            log::warn!("{} no coincide con su huella", job.url);
            return Err(Failure::Retry(GameError::corrupt()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_https_addresses_to_trusted_hosts_pass() {
        for good in [
            "https://cdn.modrinth.com/data/vvuO3ImH/versions/IPQlZkz1/curios-forge-5.14.1%2B1.20.1.jar",
            "https://piston-data.mojang.com/v1/objects/0c3ec587af28e5a785c0b4a7b8a30f9a8f78f838/client.jar",
            "https://raw.githubusercontent.com/Kevinn2709/Meca-Launcher/main/manifest.json",
        ] {
            assert!(check_url(good).is_ok(), "debería aceptar {good}");
        }
        for bad in [
            "http://cdn.modrinth.com/data/x.jar",
            "https://evil.example/x.jar",
            "https://cdn.modrinth.com.evil.example/x.jar",
            "https://cdn.modrinth.com@evil.example/x.jar",
            "https://cdn.modrinth.com:8443/x.jar",
            "https://cdn.modrinth.com\\@evil.example/x.jar",
            "https://CDN.modrinth.com/x.jar",
            "https://cdn.modrinth.com",
            "https://cdn.modrinth.com/",
            "https://cdn.modrinth.com/x .jar",
            "file:///C:/Windows/system32/x.dll",
            "",
        ] {
            assert!(check_url(bad).is_err(), "debería rechazar {bad:?}");
        }
    }

    #[test]
    fn partial_downloads_get_their_own_name() {
        assert_eq!(part_path(Path::new("mods/a.jar")), PathBuf::from("mods/a.jar.part"));
    }
}
