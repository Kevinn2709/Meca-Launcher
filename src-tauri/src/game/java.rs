//! Java 17: se descarga la misma distribución que usa el launcher oficial,
//! archivo por archivo desde los servidores de Mojang. No hace falta que
//! el jugador tenga Java instalado.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::Value;

use super::config::JAVA_RUNTIMES_URL;
use super::download::{self, Download};
use super::error::GameError;
use super::events::CheckProgress;
use super::hash::{self, Expected, is_hex};
use super::paths::{Dirs, safe_join};
use super::task;

/// Un Java ya localizado en disco.
#[derive(Debug, Clone)]
pub struct Runtime {
    home: PathBuf,
}

impl Runtime {
    fn binary(&self, windows: &str, other: &str) -> PathBuf {
        if cfg!(windows) {
            self.home.join("bin").join(windows)
        } else if cfg!(target_os = "macos") {
            self.home.join("jre.bundle/Contents/Home/bin").join(other)
        } else {
            self.home.join("bin").join(other)
        }
    }

    /// Java sin ventana de consola: el que abre el juego.
    pub fn javaw(&self) -> PathBuf {
        self.binary("javaw.exe", "java")
    }

    /// Java con salida de texto: el que ejecuta el instalador de Forge.
    pub fn java(&self) -> PathBuf {
        self.binary("java.exe", "java")
    }

    pub fn is_present(&self) -> bool {
        self.javaw().is_file() && self.java().is_file()
    }
}

/// Lo que falta para tener Java completo.
pub struct RuntimePlan {
    pub runtime: Runtime,
    pub downloads: Vec<Download>,
    /// Archivos que en Linux y macOS necesitan permiso de ejecución.
    executables: Vec<PathBuf>,
    /// Enlaces simbólicos (solo Linux y macOS): (enlace, destino).
    links: Vec<(PathBuf, String)>,
}

/// Nombre que Mojang da a este sistema en su índice de Java.
fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x64"),
        ("windows", "x86") => Some("windows-x86"),
        ("windows", "aarch64") => Some("windows-arm64"),
        ("linux", "x86_64") => Some("linux"),
        ("linux", "x86") => Some("linux-i386"),
        ("macos", "x86_64") => Some("mac-os"),
        ("macos", "aarch64") => Some("mac-os-arm64"),
        _ => None,
    }
}

pub fn home(dirs: &Dirs, component: &str) -> PathBuf {
    dirs.runtime().join(component)
}

fn manifest_path(dirs: &Dirs, component: &str) -> PathBuf {
    dirs.runtime().join(format!("{component}.json"))
}

/// Java ya instalado, si lo está (sin tocar la red).
pub fn installed(dirs: &Dirs, component: &str) -> Option<Runtime> {
    let runtime = Runtime {
        home: home(dirs, component),
    };
    (manifest_path(dirs, component).is_file() && runtime.is_present()).then_some(runtime)
}

/// Lista de archivos de este Java. Se guarda en disco: tras la primera vez
/// no hace falta volver a pedirla.
async fn file_list(
    http: &reqwest::Client,
    dirs: &Dirs,
    component: &str,
) -> Result<Value, GameError> {
    let cached = manifest_path(dirs, component);
    if let Ok(data) = fs::read(&cached) {
        if let Ok(list) = serde_json::from_slice::<Value>(&data) {
            if list.get("files").is_some_and(Value::is_object) {
                return Ok(list);
            }
        }
    }

    let platform = platform().ok_or_else(|| {
        GameError::new(
            "unsupported",
            "Mojang no distribuye Java para este sistema, así que el launcher no puede instalarlo.",
        )
    })?;
    let unexpected = || GameError::invalid("El índice de Java de Mojang");

    let index: Value = serde_json::from_slice(&download::fetch(http, JAVA_RUNTIMES_URL, None).await?)
        .map_err(|_| unexpected())?;
    let entry = index
        .get(platform)
        .and_then(|platform| platform.get(component))
        .and_then(Value::as_array)
        .and_then(|versions| versions.first())
        .and_then(|version| version.get("manifest"))
        .ok_or_else(unexpected)?;
    let url = entry.get("url").and_then(Value::as_str).ok_or_else(unexpected)?;
    let sha1 = entry
        .get("sha1")
        .and_then(Value::as_str)
        .filter(|sha1| is_hex(sha1, 40))
        .ok_or_else(unexpected)?;

    let data = download::fetch(http, url, Some(&Expected::Sha1(sha1.to_owned()))).await?;
    let list: Value = serde_json::from_slice(&data).map_err(|_| unexpected())?;
    if !list.get("files").is_some_and(Value::is_object) {
        return Err(unexpected());
    }

    if let Some(parent) = cached.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| GameError::disk("no se pudo crear la carpeta de Java", &error))?;
    }
    fs::write(&cached, &data)
        .map_err(|error| GameError::disk("no se pudo guardar la lista de Java", &error))?;
    Ok(list)
}

/// Compara la lista de Mojang con lo que hay en disco. Con `deep` se calcula
/// la huella de cada archivo en vez de fiarse de su tamaño.
pub async fn plan(
    http: &reqwest::Client,
    dirs: &Dirs,
    component: &str,
    checks: &Arc<CheckProgress>,
    deep: bool,
) -> Result<RuntimePlan, GameError> {
    if !component
        .bytes()
        .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || component.is_empty()
    {
        return Err(GameError::invalid("El archivo de versión del juego"));
    }

    let list = file_list(http, dirs, component).await?;
    let home = home(dirs, component);
    let checks = checks.clone();
    task::blocking(move || compare(&list, home, &checks, deep)).await?
}

fn compare(
    list: &Value,
    home: PathBuf,
    checks: &CheckProgress,
    deep: bool,
) -> Result<RuntimePlan, GameError> {
    let unexpected = || GameError::invalid("La lista de archivos de Java");
    let files = list.get("files").and_then(Value::as_object).ok_or_else(unexpected)?;
    checks.expect(files.len() as u64);

    let mut plan = RuntimePlan {
        runtime: Runtime { home: home.clone() },
        downloads: Vec::new(),
        executables: Vec::new(),
        links: Vec::new(),
    };

    for (relative, entry) in files {
        checks.tick();
        let dest = safe_join(&home, relative).ok_or_else(unexpected)?;
        match entry.get("type").and_then(Value::as_str) {
            Some("directory") => {
                fs::create_dir_all(&dest)
                    .map_err(|error| GameError::disk("no se pudo crear una carpeta de Java", &error))?;
            }
            Some("link") => {
                if let Some(target) = entry.get("target").and_then(Value::as_str) {
                    plan.links.push((dest, target.to_owned()));
                }
            }
            Some("file") => {
                let raw = entry
                    .get("downloads")
                    .and_then(|downloads| downloads.get("raw"))
                    .ok_or_else(unexpected)?;
                let url = raw.get("url").and_then(Value::as_str).ok_or_else(unexpected)?;
                let size = raw.get("size").and_then(Value::as_u64).ok_or_else(unexpected)?;
                let sha1 = raw
                    .get("sha1")
                    .and_then(Value::as_str)
                    .filter(|sha1| is_hex(sha1, 40))
                    .ok_or_else(unexpected)?;

                if entry.get("executable").and_then(Value::as_bool) == Some(true) {
                    plan.executables.push(dest.clone());
                }
                let expected = Expected::Sha1(sha1.to_owned());
                if !hash::is_intact(&dest, Some(size), Some(&expected), deep) {
                    plan.downloads.push(Download {
                        url: url.to_owned(),
                        dest,
                        size: Some(size),
                        hash: Some(expected),
                        label: "Java 17".to_owned(),
                    });
                }
            }
            _ => return Err(unexpected()),
        }
    }
    Ok(plan)
}

/// Últimos retoques tras las descargas. En Windows no hay nada que hacer.
pub fn finish(plan: &RuntimePlan) -> Result<(), GameError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in &plan.executables {
            fs::set_permissions(path, fs::Permissions::from_mode(0o755))
                .map_err(|error| GameError::disk("no se pudo preparar Java", &error))?;
        }
        for (link, target) in &plan.links {
            // Mojang solo usa destinos relativos, dentro del propio Java.
            if target.starts_with('/') {
                continue;
            }
            if fs::symlink_metadata(link).is_err() {
                if let Some(parent) = link.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = std::os::unix::fs::symlink(target, link);
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (&plan.executables, &plan.links);
    }

    if plan.runtime.is_present() {
        Ok(())
    } else {
        log::warn!("Java quedó incompleto en {}", plan.runtime.home.display());
        Err(GameError::new(
            "java",
            "Java no quedó bien instalado. Pulsa Reintentar para repararlo.",
        ))
    }
}
