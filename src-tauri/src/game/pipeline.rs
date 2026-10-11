//! Los dos grandes pasos de "Jugar": dejar el juego listo y abrirlo.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use super::config::{DEFAULT_JAVA_COMPONENT, EARLY_CRASH};
use super::download;
use super::error::GameError;
use super::events::{CheckProgress, DownloadProgress, Events, PlayEvent};
use super::forge;
use super::java::{self, Runtime};
use super::launch::{self, Launch, Player};
use super::manifest::Manifest;
use super::paths::Dirs;
use super::ping;
use super::sync;
use super::task;
use super::vanilla;
use super::version::{self, Environment, Version};

/// Opciones de arranque activas: entrar directo a un servidor.
const FEATURES: &[&str] = &["is_quick_play_multiplayer"];

/// El juego instalado y al día, listo para abrirse.
pub struct Ready {
    java: Runtime,
    /// Forge combinado con la versión de Minecraft de la que parte.
    version: Version,
    env: Environment,
}

/// Nota que queda en disco mientras una instalación está a medias, o cuando
/// el juego no llegó a arrancar. Si existe al empezar, se calcula la huella
/// de todos los archivos en vez de fiarse de su tamaño: es lo que hace que
/// "Reintentar" repare de verdad tras un corte de luz o un disco con fallos.
fn repair_note(dirs: &Dirs) -> PathBuf {
    dirs.cache().join("revisar-todo.txt")
}

fn request_repair(dirs: &Dirs) {
    let written = fs::create_dir_all(dirs.cache()).and_then(|()| {
        fs::write(
            repair_note(dirs),
            "La próxima vez el launcher revisará todos los archivos del juego.\n",
        )
    });
    if let Err(error) = written {
        log::warn!("no se pudo anotar la revisión pendiente: {error}");
    }
}

/// `true` si Java, Minecraft y Forge ya están instalados (sin tocar la red).
pub fn is_game_installed(dirs: &Dirs, manifest: &Manifest) -> bool {
    java::installed(dirs, DEFAULT_JAVA_COMPONENT).is_some() && forge::is_installed(dirs, manifest)
}

/// Comprueba qué falta, lo descarga, instala Forge si es la primera vez y
/// deja los mods idénticos a la lista publicada.
pub async fn prepare(
    http: &reqwest::Client,
    dirs: &Dirs,
    manifest: &Arc<Manifest>,
    events: &Arc<Events>,
) -> Result<Ready, GameError> {
    let env = Environment::current();
    let checks = Arc::new(CheckProgress::new(events.clone()));
    let deep = repair_note(dirs).is_file();
    if deep {
        log::info!("la vez anterior no terminó bien: se revisan todos los archivos");
    }

    // 1. Minecraft: su archivo de versión dice qué Java y qué archivos necesita.
    let vanilla_json = vanilla::version_json(http, dirs, &manifest.minecraft).await?;
    let vanilla = version::parse(&vanilla_json, &env, FEATURES)?;
    let component = vanilla
        .java_component
        .clone()
        .unwrap_or_else(|| DEFAULT_JAVA_COMPONENT.to_owned());

    // 2. Qué falta de Java, de Minecraft, de Forge y de los mods.
    let runtime = java::plan(http, dirs, &component, &checks, deep).await?;
    let mut downloads = runtime.downloads.clone();
    downloads.extend(vanilla::plan(http, dirs, &vanilla, &checks, deep).await?);

    // En una revisión completa también pasa Forge: su instalador comprueba
    // cada archivo que genera y rehace solo los que estén mal.
    let forge_ready = !deep && forge::is_installed(dirs, manifest);
    if !forge_ready {
        downloads.extend(forge::installer_download(dirs, manifest));
    }
    // Los mods se comprueban con su huella en cada arranque; el resto, no.
    // Por eso solo queda algo "pendiente de revisar" si se toca el resto.
    let touches_game = !downloads.is_empty() || !forge_ready;

    let instance = dirs.instance();
    let mods = {
        let (manifest, checks) = (manifest.clone(), checks.clone());
        task::blocking(move || sync::plan(&instance, &manifest, Some(&checks))).await??
    };
    downloads.extend(mods.downloads);
    checks.finish();

    // 3. Fuera lo que sobra y a descargar lo que falta.
    let extras = mods.extras;
    task::blocking(move || sync::remove_extras(&extras)).await??;

    if touches_game {
        request_repair(dirs);
    }
    if !downloads.is_empty() {
        let total = downloads.iter().filter_map(|download| download.size).sum();
        let progress = Arc::new(DownloadProgress::new(events.clone(), total));
        download::run(http, downloads, Some(progress)).await?;
    }
    java::finish(&runtime)?;

    // 4. Forge, solo la primera vez (o si quedó a medias).
    if !forge_ready {
        events.send(PlayEvent::Installing);
        forge::install(dirs, manifest, runtime.runtime.java()).await?;
    }
    let forge_id = forge::version_id(&manifest.minecraft, &manifest.forge.version);
    let forge_json = forge::version_json(dirs, &forge_id)
        .ok_or_else(|| GameError::invalid("El archivo de versión de Forge"))?;
    let forge = version::parse(&forge_json, &env, FEATURES)?;

    // Si se borró alguna librería de Forge, se repone sin reinstalarlo todo.
    let repairs = {
        let (dirs, libraries) = (dirs.clone(), forge.libraries.clone());
        task::blocking(move || vanilla::missing_libraries(&dirs, &libraries, "Forge", None, deep))
            .await??
    };
    if !repairs.is_empty() {
        download::run(http, repairs, None).await?;
    }

    sync::record_installed(&dirs.instance(), manifest)?;
    // Todo en su sitio y comprobado: ya no hay nada pendiente de revisar.
    let _ = fs::remove_file(repair_note(dirs));
    Ok(Ready {
        java: runtime.runtime,
        version: version::merge(forge, vanilla),
        env,
    })
}

/// Abre Minecraft y espera a que el jugador lo cierre.
pub async fn launch(
    dirs: &Dirs,
    manifest: &Manifest,
    ready: &Ready,
    player: Player<'_>,
    launcher_version: &str,
    events: &Events,
) -> Result<(), GameError> {
    events.send(PlayEvent::Launching);

    // Por la red de casa si responde ahí; si no, por internet.
    let server = manifest.server.clone();
    let host = task::blocking(move || {
        ping::locate(&server)
            .map(|(host, _)| host)
            .unwrap_or(server.address)
    })
    .await?;
    let target = format!("{host}:{}", manifest.server.port);

    launch::place_client(dirs, &ready.version)?;
    let arguments = launch::arguments(&Launch {
        version: &ready.version,
        dirs,
        env: ready.env,
        player,
        server: Some(&target),
        memory: launch::memory(),
        launcher_version,
    })?;

    let log = launch::game_log(dirs);
    let started = Instant::now();
    let spawned = launch::spawn(&ready.java.javaw(), &arguments, dirs, &ready.version, &log);
    // Los argumentos llevan el token de sesión: se sueltan cuanto antes.
    drop(arguments);
    let mut child = spawned.inspect_err(|_| request_repair(dirs))?;
    log::info!("Minecraft abierto, entrando a {target}");
    events.send(PlayEvent::Running);

    let status = task::blocking(move || child.wait())
        .await?
        .map_err(|error| GameError::disk("se perdió el rastro de Minecraft", &error))?;
    log::info!("Minecraft se cerró: {status}");
    if status.success() {
        return Ok(());
    }
    // Si se cerró nada más abrirse puede haber un archivo dañado: la próxima
    // vez se revisan todos. Un cierre tras un rato de juego no lo necesita.
    let early = started.elapsed() < EARLY_CRASH;
    if early {
        request_repair(dirs);
    }
    Err(GameError::new(
        "game_crashed",
        format!(
            "Minecraft se cerró con un error.{} El detalle está en {}",
            if early {
                " Al pulsar Reintentar se revisarán todos los archivos del juego."
            } else {
                ""
            },
            log.display()
        ),
    ))
}
