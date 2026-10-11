//! Forge: se instala con su instalador oficial, sin ventanas, sobre la
//! carpeta de Minecraft del launcher. Solo hace falta la primera vez.
//!
//! El instalador descarga las librerías de Forge y aplica sus parches al
//! cliente de Minecraft; por eso tarda uno o dos minutos.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::Value;

use super::config::FORGE_INSTALL_TIMEOUT;
use super::download::Download;
use super::error::GameError;
use super::hash::{self, Expected};
use super::launch;
use super::manifest::Manifest;
use super::paths::{Dirs, is_safe_component, safe_join};
use super::task;
use super::version::argument_value;

/// Nombre que el instalador da a la versión: "1.20.1-forge-47.4.10".
pub fn version_id(minecraft: &str, forge: &str) -> String {
    format!("{minecraft}-forge-{forge}")
}

fn installer_path(dirs: &Dirs, manifest: &Manifest) -> PathBuf {
    dirs.cache().join(format!(
        "forge-{}-{}-installer.jar",
        manifest.minecraft, manifest.forge.version
    ))
}

fn log_path(dirs: &Dirs) -> PathBuf {
    dirs.cache().join("forge-installer.log")
}

/// Nota que deja el launcher cuando el instalador termina bien. Guarda qué
/// archivos generó, para comprobar después que siguen en su sitio.
fn receipt_path(dirs: &Dirs, id: &str) -> PathBuf {
    dirs.versions().join(id).join("instalado-por-meca.txt")
}

/// Archivo de versión que deja el instalador, si existe y es el esperado.
pub fn version_json(dirs: &Dirs, id: &str) -> Option<Value> {
    let data = fs::read(dirs.version_json(id)).ok()?;
    let json: Value = serde_json::from_slice(&data).ok()?;
    (json.get("id").and_then(Value::as_str) == Some(id)).then_some(json)
}

/// Archivos que Forge necesita para arrancar y que no figuran en su archivo
/// de versión: los que el instalador genera en este equipo (Forge aplicado
/// sobre el cliente) y los que descarga por su cuenta. Rutas dentro de
/// `libraries/`, con `/`. Es la disposición de Forge para Minecraft 1.17 a 1.20.
fn expected_files(minecraft: &str, forge: &str, json: &Value) -> Vec<String> {
    let build = format!("{minecraft}-{forge}");
    let mut files = vec![
        format!("net/minecraftforge/forge/{build}/forge-{build}-client.jar"),
        format!("net/minecraftforge/forge/{build}/forge-{build}-universal.jar"),
    ];
    for part in ["fmlcore", "javafmllanguage", "lowcodelanguage", "mclanguage"] {
        files.push(format!("net/minecraftforge/{part}/{build}/{part}-{build}.jar"));
    }

    let game_arguments: Vec<String> = json
        .get("arguments")
        .and_then(|arguments| arguments.get("game"))
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    if let Some(mcp) = argument_value(&game_arguments, "--fml.mcpVersion").filter(|mcp| is_safe_component(mcp)) {
        let client = format!("{minecraft}-{mcp}");
        files.push(format!("net/minecraft/client/{client}/client-{client}-srg.jar"));
        files.push(format!("net/minecraft/client/{client}/client-{client}-extra.jar"));
    }
    files
}

fn is_nonempty_file(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
}

/// `true` si el instalador terminó bien alguna vez y lo que dejó sigue ahí.
/// El instalador escribe su archivo de versión nada más empezar, así que
/// ese archivo por sí solo no demuestra que la instalación se completó.
pub fn is_installed(dirs: &Dirs, manifest: &Manifest) -> bool {
    let id = version_id(&manifest.minecraft, &manifest.forge.version);
    if version_json(dirs, &id).is_none() {
        return false;
    }
    let Ok(receipt) = fs::read_to_string(receipt_path(dirs, &id)) else {
        return false;
    };
    let libraries = dirs.libraries();
    let mut files = receipt.lines().filter(|line| !line.is_empty()).peekable();
    files.peek().is_some()
        && files.all(|line| safe_join(&libraries, line).is_some_and(|file| is_nonempty_file(&file)))
}

/// Anota la instalación terminada, con los archivos que dejó el instalador.
/// `Ok(false)` si falta lo imprescindible: Forge aplicado sobre el cliente
/// y el propio Forge. Sin esos dos archivos el juego no arranca, diga lo que
/// diga el instalador (un antivirus puede haberlos retirado).
fn write_receipt(dirs: &Dirs, manifest: &Manifest) -> Result<bool, GameError> {
    let id = version_id(&manifest.minecraft, &manifest.forge.version);
    let Some(json) = version_json(dirs, &id) else {
        return Ok(false);
    };
    let libraries = dirs.libraries();
    let exists = |file: &String| safe_join(&libraries, file).is_some_and(|path| is_nonempty_file(&path));

    let expected = expected_files(&manifest.minecraft, &manifest.forge.version, &json);
    if !expected.iter().take(2).all(exists) {
        return Ok(false);
    }
    let present: Vec<String> = expected.into_iter().filter(exists).collect();
    fs::write(receipt_path(dirs, &id), present.join("\n"))
        .map_err(|error| GameError::disk("no se pudo anotar la instalación de Forge", &error))?;
    Ok(true)
}

/// Descarga del instalador, si hace falta. Se guarda con su huella SHA-1
/// comprobada: es código que se va a ejecutar.
pub fn installer_download(dirs: &Dirs, manifest: &Manifest) -> Option<Download> {
    let dest = installer_path(dirs, manifest);
    let expected = Expected::Sha1(manifest.forge.installer_sha1.clone());
    if hash::file_matches(&dest, &expected).unwrap_or(false) {
        return None;
    }
    Some(Download {
        url: manifest.forge.installer_url.clone(),
        dest,
        size: None,
        hash: Some(expected),
        label: format!("Forge {}", manifest.forge.version),
    })
}

/// Ejecuta el instalador y comprueba que dejó todo en su sitio.
pub async fn install(dirs: &Dirs, manifest: &Manifest, java: PathBuf) -> Result<(), GameError> {
    let installer = installer_path(dirs, manifest);
    let minecraft = dirs.minecraft();
    let cache = dirs.cache();
    let log = log_path(dirs);

    // El instalador se vuelve a comprobar justo antes de ejecutarlo.
    let expected = Expected::Sha1(manifest.forge.installer_sha1.clone());
    let failure = install_failure(&log);
    let status = task::blocking(move || {
        if !hash::file_matches(&installer, &expected).unwrap_or(false) {
            return Err(GameError::corrupt());
        }
        run_installer(&java, &installer, &minecraft, &cache, &log)
    })
    .await??;

    if !status {
        return Err(failure);
    }
    if !write_receipt(dirs, manifest)? || !is_installed(dirs, manifest) {
        log::warn!("el instalador de Forge terminó, pero no dejó todos sus archivos");
        return Err(failure);
    }
    Ok(())
}

fn install_failure(log: &Path) -> GameError {
    GameError::new(
        "forge_install",
        format!(
            "No se pudo instalar Forge. Revisa tu conexión y pulsa Reintentar. El detalle quedó en {}",
            log.display()
        ),
    )
}

/// `Ok(true)` si el instalador terminó bien.
fn run_installer(
    java: &Path,
    installer: &Path,
    minecraft: &Path,
    cache: &Path,
    log: &Path,
) -> Result<bool, GameError> {
    let disk = |context: &'static str| move |error: std::io::Error| GameError::disk(context, &error);

    fs::create_dir_all(minecraft).map_err(disk("no se pudo crear la carpeta de Minecraft"))?;
    fs::create_dir_all(cache).map_err(disk("no se pudo crear la carpeta de trabajo"))?;

    // El instalador solo acepta carpetas que parezcan del launcher oficial:
    // busca este archivo y anota en él un perfil que aquí no se usa.
    let profiles = minecraft.join("launcher_profiles.json");
    if !profiles.is_file() {
        fs::write(&profiles, "{\"profiles\": {}}\n")
            .map_err(disk("no se pudo preparar la carpeta de Minecraft"))?;
    }

    let output = File::create(log).map_err(disk("no se pudo crear el registro de Forge"))?;
    let errors = output
        .try_clone()
        .map_err(disk("no se pudo crear el registro de Forge"))?;

    let mut command = Command::new(java);
    command
        .arg("-jar")
        .arg(installer)
        .arg("--installClient")
        .arg(minecraft)
        .current_dir(cache)
        .stdin(Stdio::null())
        .stdout(Stdio::from(output))
        .stderr(Stdio::from(errors));
    launch::clean_environment(&mut command);
    launch::hide_console(&mut command);

    let mut child = command.spawn().map_err(|error| {
        log::warn!("no se pudo ejecutar el instalador de Forge: {error}");
        GameError::new("java", "No se pudo ejecutar Java. Pulsa Reintentar para repararlo.")
    })?;

    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                log::info!("el instalador de Forge terminó: {status}");
                return Ok(status.success());
            }
            Ok(None) if started.elapsed() > FORGE_INSTALL_TIMEOUT => {
                log::warn!("el instalador de Forge tardó demasiado y se detuvo");
                let _ = child.kill();
                let _ = child.wait();
                return Ok(false);
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(250)),
            Err(error) => {
                log::warn!("se perdió el rastro del instalador de Forge: {error}");
                return Ok(false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::manifest::{ForgeSpec, ServerSpec};

    fn manifest() -> Manifest {
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
            files: Vec::new(),
        }
    }

    #[test]
    fn an_install_without_its_main_files_does_not_count() {
        let root = std::env::temp_dir().join(format!("meca-forge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dirs = Dirs::new(root.clone());
        let manifest = manifest();
        let id = version_id("1.20.1", "47.4.10");
        assert_eq!(id, "1.20.1-forge-47.4.10");

        // Nada instalado.
        assert!(!is_installed(&dirs, &manifest));
        assert!(!write_receipt(&dirs, &manifest).unwrap());

        // El instalador dejó su archivo de versión, pero ningún otro archivo
        // (por ejemplo, porque un antivirus los retiró): no vale.
        fs::create_dir_all(dirs.versions().join(&id)).unwrap();
        fs::write(
            dirs.version_json(&id),
            r#"{"id": "1.20.1-forge-47.4.10", "arguments": {"game": ["--fml.mcpVersion", "20230612.114412"]}}"#,
        )
        .unwrap();
        assert!(!write_receipt(&dirs, &manifest).unwrap());
        assert!(!is_installed(&dirs, &manifest));
        // Una nota vacía tampoco demuestra nada.
        fs::write(receipt_path(&dirs, &id), "").unwrap();
        assert!(!is_installed(&dirs, &manifest));

        // Con Forge y el cliente con Forge aplicado en su sitio, sí.
        let json = version_json(&dirs, &id).unwrap();
        let expected = expected_files("1.20.1", "47.4.10", &json);
        assert_eq!(expected.len(), 8);
        for file in &expected[..2] {
            let path = safe_join(&dirs.libraries(), file).unwrap();
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"jar").unwrap();
        }
        assert!(write_receipt(&dirs, &manifest).unwrap());
        assert!(is_installed(&dirs, &manifest));

        // Si después desaparece uno, deja de contar como instalado.
        fs::remove_file(safe_join(&dirs.libraries(), &expected[0]).unwrap()).unwrap();
        assert!(!is_installed(&dirs, &manifest));
        let _ = fs::remove_dir_all(&root);
    }
}
