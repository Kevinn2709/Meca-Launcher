//! Sincronización del modpack: deja la carpeta del juego igual que la
//! lista publicada. Descarga lo que falta o cambió y quita de las carpetas
//! administradas (los mods) lo que ya no pertenece al modpack.
//!
//! Hay dos clases de archivos:
//!   - Los de las carpetas administradas (mods) deben ser idénticos a la
//!     lista: se comprueba su huella en cada arranque.
//!   - El resto (configuración) se instala cuando falta o cuando el modpack
//!     publica una versión nueva. Entre tanto es del jugador: el juego y
//!     los mods lo reescriben, y el launcher no lo vuelve a pisar.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::download::Download;
use super::error::GameError;
use super::events::CheckProgress;
use super::hash::{self, Expected};
use super::manifest::Manifest;
use super::paths::safe_join;

/// Diferencias entre la lista publicada y la carpeta local.
#[derive(Debug, Default)]
pub struct SyncPlan {
    pub downloads: Vec<Download>,
    /// Archivos que sobran en las carpetas administradas.
    pub extras: Vec<PathBuf>,
    /// Bytes por descargar.
    pub bytes: u64,
}

impl SyncPlan {
    /// Archivos que cambian al sincronizar: los que se bajan y los que se quitan.
    pub fn pending(&self) -> u64 {
        (self.downloads.len() + self.extras.len()) as u64
    }
}

/// Nombre del archivo donde se anota la versión del modpack instalada.
const VERSION_FILE: &str = "modpack-version.txt";

/// Nombre del archivo donde se anota qué versión de cada archivo de
/// configuración se instaló: una línea "huella ruta" por archivo.
const INSTALLED_FILE: &str = "modpack-files.txt";

/// Versión del modpack con la que se sincronizó por última vez.
pub fn installed_version(instance: &Path) -> Option<String> {
    let text = fs::read_to_string(instance.join(VERSION_FILE)).ok()?;
    let version = text.trim();
    (!version.is_empty() && version.len() <= 40).then(|| version.to_owned())
}

/// Huella publicada de cada archivo de configuración que ya se instaló.
fn installed_files(instance: &Path) -> HashMap<String, String> {
    let Ok(text) = fs::read_to_string(instance.join(INSTALLED_FILE)) else {
        return HashMap::new();
    };
    text.lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(sha512, path)| (path.to_owned(), sha512.to_owned()))
        .collect()
}

/// Anota la sincronización terminada: versión del modpack y archivos de
/// configuración instalados.
pub fn record_installed(instance: &Path, manifest: &Manifest) -> Result<(), GameError> {
    let disk = |error| GameError::disk("no se pudo anotar la versión del modpack", &error);
    fs::create_dir_all(instance).map_err(disk)?;

    let mut lines: Vec<String> = manifest
        .files
        .iter()
        .filter(|file| !manifest.is_managed(&file.path))
        .map(|file| format!("{} {}", file.sha512, file.path))
        .collect();
    lines.sort();
    fs::write(instance.join(INSTALLED_FILE), lines.join("\n")).map_err(disk)?;
    fs::write(instance.join(VERSION_FILE), &manifest.version).map_err(disk)
}

/// Recorre `dir` y apunta cada archivo con su ruta relativa (con `/`).
/// No entra en enlaces a otras carpetas.
fn walk(dir: &Path, prefix: &str, found: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let kind = entry.file_type()?;
        let relative = format!("{prefix}/{}", entry.file_name().to_string_lossy());
        let path = entry.path();
        if kind.is_dir() {
            walk(&path, &relative, found)?;
        } else if kind.is_symlink() && path.is_dir() {
            continue;
        } else {
            found.push((relative, path));
        }
    }
    Ok(())
}

/// `true` si el archivo local es exactamente el publicado.
fn is_published(dest: &Path, size: u64, expected: &Expected) -> bool {
    hash::is_intact(dest, Some(size), Some(expected), true)
}

/// Compara la carpeta del juego con la lista. Lee y calcula la huella de
/// cada archivo local, así que conviene llamarla fuera de los hilos de red.
pub fn plan(
    instance: &Path,
    manifest: &Manifest,
    checks: Option<&CheckProgress>,
) -> Result<SyncPlan, GameError> {
    let mut plan = SyncPlan::default();
    if let Some(checks) = checks {
        checks.expect(manifest.files.len() as u64);
    }

    let installed = installed_files(instance);
    for file in &manifest.files {
        let dest = safe_join(instance, &file.path).ok_or_else(GameError::internal)?;
        let expected = Expected::Sha512(file.sha512.clone());
        let wanted = if manifest.is_managed(&file.path) {
            !is_published(&dest, file.size, &expected)
        } else {
            // Configuración: solo si falta o si el modpack publicó otra versión
            // distinta de la que ya se instaló aquí.
            !dest.is_file()
                || (installed.get(&file.path) != Some(&file.sha512)
                    && !is_published(&dest, file.size, &expected))
        };
        if wanted {
            plan.bytes += file.size;
            plan.downloads.push(Download {
                url: file.url.clone(),
                dest,
                size: Some(file.size),
                hash: Some(expected),
                label: file.path.rsplit('/').next().unwrap_or(&file.path).to_owned(),
            });
        }
        if let Some(checks) = checks {
            checks.tick();
        }
    }

    // Windows no distingue mayúsculas en los nombres: se compara sin ellas.
    let wanted: HashSet<String> = manifest.files.iter().map(|file| file.path.to_lowercase()).collect();
    for folder in &manifest.managed_folders {
        let dir = safe_join(instance, folder).ok_or_else(GameError::internal)?;
        let mut found = Vec::new();
        walk(&dir, folder, &mut found)
            .map_err(|error| GameError::disk("no se pudo leer una carpeta del juego", &error))?;
        for (relative, path) in found {
            if !wanted.contains(&relative.to_lowercase()) {
                plan.extras.push(path);
            }
        }
    }
    Ok(plan)
}

/// Quita los archivos que sobran.
pub fn remove_extras(extras: &[PathBuf]) -> Result<(), GameError> {
    for path in extras {
        match fs::remove_file(path) {
            Ok(()) => log::info!("se quitó {}, que ya no es parte del modpack", path.display()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                log::warn!("no se pudo quitar {}: {error}", path.display());
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned());
                return Err(GameError::new(
                    "disk",
                    format!(
                        "No se pudo quitar {}, que ya no es parte del modpack. Cierra Minecraft si está abierto e inténtalo de nuevo.",
                        name.as_deref().unwrap_or("un archivo")
                    ),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::hash::Hasher;
    use crate::game::manifest::{ForgeSpec, ManifestFile, ServerSpec};

    fn published(path: &str, content: &[u8]) -> ManifestFile {
        let expected = Expected::Sha512(String::new());
        let mut hasher = Hasher::matching(&expected);
        hasher.update(content);
        ManifestFile {
            path: path.into(),
            size: content.len() as u64,
            sha512: hasher.finish(),
            url: format!("https://cdn.modrinth.com/data/x/{path}"),
        }
    }

    fn manifest(files: Vec<ManifestFile>) -> Manifest {
        Manifest {
            name: "Meca".into(),
            version: "1.0.0".into(),
            minecraft: "1.20.1".into(),
            forge: ForgeSpec {
                version: "47.4.10".into(),
                installer_url: String::new(),
                installer_sha1: String::new(),
            },
            server: ServerSpec {
                address: "example.org".into(),
                port: 25565,
                lan_address: None,
            },
            managed_folders: vec!["mods".into()],
            files,
        }
    }

    #[test]
    fn mods_must_match_but_settings_belong_to_the_player() {
        let instance = std::env::temp_dir().join(format!("meca-sync-{}", std::process::id()));
        let _ = fs::remove_dir_all(&instance);
        fs::create_dir_all(instance.join("mods")).unwrap();
        fs::create_dir_all(instance.join("config")).unwrap();
        let pending = |manifest: &Manifest| -> Vec<String> {
            let plan = plan(&instance, manifest, None).unwrap();
            plan.downloads.into_iter().map(|download| download.label).collect()
        };

        let first = manifest(vec![published("mods/a.jar", b"mod v1"), published("config/a.toml", b"x = 1")]);
        assert_eq!(pending(&first), ["a.jar", "a.toml"], "nothing is there yet");

        // Instalado tal cual se publicó.
        fs::write(instance.join("mods/a.jar"), b"mod v1").unwrap();
        fs::write(instance.join("config/a.toml"), b"x = 1").unwrap();
        record_installed(&instance, &first).unwrap();
        assert!(pending(&first).is_empty());
        assert_eq!(installed_version(&instance).as_deref(), Some("1.0.0"));

        // El jugador (o el propio mod) cambia la configuración: se respeta.
        // Un mod cambiado, en cambio, se repone siempre.
        fs::write(instance.join("config/a.toml"), b"x = 2 # mi ajuste").unwrap();
        fs::write(instance.join("mods/a.jar"), b"mod trucado").unwrap();
        assert_eq!(pending(&first), ["a.jar"]);

        // El modpack publica una configuración nueva: esa sí se instala.
        let second = manifest(vec![published("mods/a.jar", b"mod v1"), published("config/a.toml", b"x = 3")]);
        assert_eq!(pending(&second), ["a.jar", "a.toml"]);

        // Si la configuración se borra, vuelve la publicada.
        fs::remove_file(instance.join("config/a.toml")).unwrap();
        assert_eq!(pending(&first), ["a.jar", "a.toml"]);

        // Un archivo que ya coincide no se descarga aunque no esté anotado.
        fs::remove_file(instance.join(INSTALLED_FILE)).unwrap();
        fs::write(instance.join("config/a.toml"), b"x = 3").unwrap();
        fs::write(instance.join("mods/a.jar"), b"mod v1").unwrap();
        assert!(pending(&second).is_empty());

        let _ = fs::remove_dir_all(&instance);
    }
}
