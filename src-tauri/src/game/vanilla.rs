//! Minecraft sin modificar: archivo de versión, cliente, librerías y
//! recursos (sonidos, idiomas, texturas), todo desde los servidores de Mojang.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use super::config::{ASSETS_URL, VERSION_MANIFEST_URL};
use super::download::{self, Download};
use super::error::GameError;
use super::events::CheckProgress;
use super::hash::{self, Expected, is_hex};
use super::paths::{Dirs, safe_join};
use super::task;
use super::version::{Library, Remote, Version};

fn disk(context: &'static str) -> impl Fn(std::io::Error) -> GameError {
    move |error| GameError::disk(context, &error)
}

fn save(path: &Path, data: &[u8]) -> Result<(), GameError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(disk("no se pudo crear una carpeta del juego"))?;
    }
    fs::write(path, data).map_err(disk("no se pudo guardar un archivo del juego"))
}

/// Archivo de versión de Minecraft. Tras la primera vez se lee del disco.
pub async fn version_json(
    http: &reqwest::Client,
    dirs: &Dirs,
    id: &str,
) -> Result<Value, GameError> {
    let cached = dirs.version_json(id);
    if let Ok(data) = fs::read(&cached) {
        if let Ok(json) = serde_json::from_slice::<Value>(&data) {
            if json.get("id").and_then(Value::as_str) == Some(id) {
                return Ok(json);
            }
        }
    }

    let unexpected = || GameError::invalid("El índice de versiones de Minecraft");
    let index: Value =
        serde_json::from_slice(&download::fetch(http, VERSION_MANIFEST_URL, None).await?)
            .map_err(|_| unexpected())?;
    let entry = index
        .get("versions")
        .and_then(Value::as_array)
        .and_then(|versions| {
            versions
                .iter()
                .find(|version| version.get("id").and_then(Value::as_str) == Some(id))
        })
        .ok_or_else(|| {
            GameError::new(
                "unknown_version",
                format!("Mojang no tiene publicada la versión {id} de Minecraft."),
            )
        })?;
    let url = entry.get("url").and_then(Value::as_str).ok_or_else(unexpected)?;
    let sha1 = entry
        .get("sha1")
        .and_then(Value::as_str)
        .filter(|sha1| is_hex(sha1, 40))
        .ok_or_else(unexpected)?;

    let data = download::fetch(http, url, Some(&Expected::Sha1(sha1.to_owned()))).await?;
    let json: Value = serde_json::from_slice(&data).map_err(|_| unexpected())?;
    if json.get("id").and_then(Value::as_str) != Some(id) {
        return Err(unexpected());
    }
    save(&cached, &data)?;
    Ok(json)
}

/// Descarga de un archivo remoto de Mojang o Forge hacia `dest`, si falta
/// (o si, con `deep`, su huella no es la publicada).
fn job(remote: &Remote, dest: PathBuf, label: &str, deep: bool) -> Option<Download> {
    let expected = remote.sha1.clone().map(Expected::Sha1);
    let present = hash::is_intact(&dest, remote.size, expected.as_ref(), deep);
    (!present).then(|| Download {
        url: remote.url.clone(),
        dest,
        size: remote.size,
        hash: expected,
        label: label.to_owned(),
    })
}

/// Librerías que faltan en disco y se pueden descargar. Lee del disco:
/// conviene llamarla fuera de los hilos de red.
pub fn missing_libraries(
    dirs: &Dirs,
    libraries: &[Library],
    label: &str,
    checks: Option<&CheckProgress>,
    deep: bool,
) -> Result<Vec<Download>, GameError> {
    let root = dirs.libraries();
    if let Some(checks) = checks {
        checks.expect(libraries.len() as u64);
    }
    let mut downloads = Vec::new();
    for library in libraries {
        if let Some(checks) = checks {
            checks.tick();
        }
        let dest = safe_join(&root, &library.path)
            .ok_or_else(|| GameError::invalid("Una librería del juego"))?;
        if let Some(remote) = &library.download {
            downloads.extend(job(remote, dest, label, deep));
        }
    }
    Ok(downloads)
}

/// Índice de recursos: qué archivo corresponde a cada sonido, idioma o textura.
async fn asset_index(
    http: &reqwest::Client,
    dirs: &Dirs,
    version: &Version,
) -> Result<Value, GameError> {
    let unexpected = || GameError::invalid("El índice de recursos del juego");
    let index = version.asset_index.as_ref().ok_or_else(unexpected)?;
    let path = safe_join(&dirs.assets().join("indexes"), &format!("{}.json", index.id))
        .ok_or_else(unexpected)?;

    if let Ok(data) = fs::read(&path) {
        if let Ok(json) = serde_json::from_slice::<Value>(&data) {
            if json.get("objects").is_some_and(Value::is_object) {
                return Ok(json);
            }
        }
    }

    let expected = index.download.sha1.clone().map(Expected::Sha1);
    let data = download::fetch(http, &index.download.url, expected.as_ref()).await?;
    let json: Value = serde_json::from_slice(&data).map_err(|_| unexpected())?;
    if !json.get("objects").is_some_and(Value::is_object) {
        return Err(unexpected());
    }
    save(&path, &data)?;
    Ok(json)
}

fn missing_assets(
    index: &Value,
    objects_dir: PathBuf,
    checks: &CheckProgress,
    deep: bool,
) -> Result<Vec<Download>, GameError> {
    let unexpected = || GameError::invalid("El índice de recursos del juego");
    let objects = index.get("objects").and_then(Value::as_object).ok_or_else(unexpected)?;
    checks.expect(objects.len() as u64);

    let mut downloads = Vec::new();
    // Varios recursos pueden compartir el mismo archivo: se baja una sola vez.
    let mut seen = HashSet::new();
    for (_, object) in objects {
        checks.tick();
        let hash = object
            .get("hash")
            .and_then(Value::as_str)
            .filter(|hash| is_hex(hash, 40))
            .ok_or_else(unexpected)?
            .to_ascii_lowercase();
        let size = object.get("size").and_then(Value::as_u64).ok_or_else(unexpected)?;
        if !seen.insert(hash.clone()) {
            continue;
        }

        let prefix = &hash[..2];
        let dest = objects_dir.join(prefix).join(&hash);
        let url = format!("{ASSETS_URL}/{prefix}/{hash}");
        let expected = Expected::Sha1(hash);
        if !hash::is_intact(&dest, Some(size), Some(&expected), deep) {
            downloads.push(Download {
                url,
                dest,
                size: Some(size),
                hash: Some(expected),
                label: "Recursos de Minecraft".to_owned(),
            });
        }
    }
    Ok(downloads)
}

/// Todo lo que falta de Minecraft: cliente, librerías y recursos.
pub async fn plan(
    http: &reqwest::Client,
    dirs: &Dirs,
    version: &Version,
    checks: &Arc<CheckProgress>,
    deep: bool,
) -> Result<Vec<Download>, GameError> {
    let label = format!("Minecraft {}", version.id);
    let client = version
        .client
        .clone()
        .ok_or_else(|| GameError::invalid("El archivo de versión del juego"))?;
    let index = asset_index(http, dirs, version).await?;

    // Comparar con el disco bloquea: va en un hilo aparte de los de red.
    let jar = dirs.version_jar(&version.id);
    let (dirs, libraries, checks) = (dirs.clone(), version.libraries.clone(), checks.clone());
    task::blocking(move || {
        let mut downloads = Vec::new();
        checks.expect(1);
        downloads.extend(job(&client, jar, &label, deep));
        checks.tick();
        downloads.extend(missing_libraries(&dirs, &libraries, &label, Some(&checks), deep)?);
        downloads.extend(missing_assets(&index, dirs.assets().join("objects"), &checks, deep)?);
        Ok(downloads)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::events::Events;

    #[test]
    fn shared_assets_are_downloaded_once() {
        let index: Value = serde_json::from_str(
            r#"{"objects": {
                "minecraft/sounds/a.ogg": {"hash": "bdf48ef6b5d0d23bbb02e17d04865216179f510a", "size": 3665},
                "minecraft/sounds/b.ogg": {"hash": "bdf48ef6b5d0d23bbb02e17d04865216179f510a", "size": 3665},
                "minecraft/lang/es_mx.json": {"hash": "7d1b5b0b2b0b2f5b3a0c4b5d6e7f8091a2b3c4d5", "size": 400}
            }}"#,
        )
        .unwrap();
        let checks = CheckProgress::new(Arc::new(Events::new(|_| {})));
        let downloads = missing_assets(&index, PathBuf::from("no-existe/objects"), &checks, false).unwrap();
        assert_eq!(downloads.len(), 2);
        let sound = downloads.iter().find(|d| d.size == Some(3665)).unwrap();
        assert_eq!(
            sound.url,
            "https://resources.download.minecraft.net/bd/bdf48ef6b5d0d23bbb02e17d04865216179f510a"
        );
        assert!(sound.dest.ends_with("bd/bdf48ef6b5d0d23bbb02e17d04865216179f510a"));
    }

    #[test]
    fn rejects_an_asset_with_a_strange_name() {
        let index: Value =
            serde_json::from_str(r#"{"objects": {"x": {"hash": "../../../evil", "size": 1}}}"#).unwrap();
        let checks = CheckProgress::new(Arc::new(Events::new(|_| {})));
        assert!(missing_assets(&index, PathBuf::from("objects"), &checks, false).is_err());
    }
}
