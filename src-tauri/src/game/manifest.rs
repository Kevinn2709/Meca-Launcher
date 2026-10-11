//! Lista de archivos del modpack (`manifest.json` de tu repositorio).
//!
//! Llega de internet, así que cada campo se comprueba antes de usarlo:
//! rutas que no puedan salirse de la carpeta del juego, descargas solo de
//! servidores de confianza y huellas con la forma correcta.

use std::collections::HashSet;

use serde_json::Value;

use super::config::FORGE_HOST;
use super::download::{check_url, https_host};
use super::error::GameError;
use super::hash::is_hex;
use super::paths::is_safe_component;

/// Versión del formato que entiende este launcher.
const FORMAT_VERSION: u64 = 1;

/// Carpetas que el launcher puede mantener idénticas a la lista (borrando lo que sobre).
const MANAGEABLE: &[&str] = &["mods", "resourcepacks", "shaderpacks"];

/// Únicas carpetas de la carpeta del juego en las que la lista puede poner
/// archivos. Todo lo demás (mundos, capturas, registros…) es del jugador.
const WRITABLE_FOLDERS: &[&str] = &[
    "mods",
    "config",
    "defaultconfigs",
    "resourcepacks",
    "shaderpacks",
    "kubejs",
];

/// Únicos archivos sueltos que la lista puede poner junto a esas carpetas:
/// opciones iniciales y lista de servidores.
const WRITABLE_FILES: &[&str] = &["options.txt", "servers.dat"];

const MAX_FILES: usize = 5000;
const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub minecraft: String,
    pub forge: ForgeSpec,
    pub server: ServerSpec,
    pub managed_folders: Vec<String>,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForgeSpec {
    pub version: String,
    pub installer_url: String,
    pub installer_sha1: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServerSpec {
    /// Dirección para entrar desde internet.
    pub address: String,
    pub port: u16,
    /// Dirección dentro de la red de casa, si la hay. Se prueba primero.
    pub lan_address: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ManifestFile {
    /// Ruta dentro de la carpeta del juego, con `/`.
    pub path: String,
    pub size: u64,
    pub sha512: String,
    pub url: String,
}

fn problem(detail: &str) -> GameError {
    log::warn!("lista del modpack no válida: {detail}");
    GameError::new(
        "bad_manifest",
        format!("La lista del modpack publicada tiene un error ({detail}). Avisa a quien administra el servidor."),
    )
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, GameError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| problem(&format!("falta \"{key}\"")))
}

/// Números separados por puntos: "1.20.1", "47.4.10".
fn is_version_number(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 32
        && text
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Nombre de servidor o IPv4: letras, números, puntos y guiones.
fn is_host_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 253
        && !text.starts_with(['.', '-'])
        && !text.ends_with(['.', '-'])
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
}

impl Manifest {
    /// `true` si `path` está en una carpeta que debe quedar idéntica a la
    /// lista. Lo demás (configuración) se instala y luego es del jugador.
    pub fn is_managed(&self, path: &str) -> bool {
        let folder = path.split('/').next().unwrap_or_default();
        self.managed_folders.iter().any(|managed| managed == folder)
    }
}

pub fn parse(data: &[u8]) -> Result<Manifest, GameError> {
    // El Bloc de notas de Windows a veces guarda una marca invisible al
    // principio del archivo; se ignora en vez de dar el archivo por malo.
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    let root: Value = serde_json::from_slice(data).map_err(|_| problem("no es JSON válido"))?;

    match root.get("formatVersion").and_then(Value::as_u64) {
        Some(FORMAT_VERSION) => {}
        Some(newer) if newer > FORMAT_VERSION => {
            return Err(GameError::new(
                "outdated_launcher",
                "El modpack necesita una versión más nueva del launcher. Pide el instalador actualizado.",
            ));
        }
        _ => return Err(problem("falta \"formatVersion\"")),
    }

    let name = text(&root, "name")?.to_owned();
    let version = text(&root, "version")?.to_owned();
    if version.is_empty() || version.len() > 40 || version.chars().any(char::is_control) {
        return Err(problem("\"version\" no es válida"));
    }
    let minecraft = text(&root, "minecraft")?.to_owned();
    if !is_version_number(&minecraft) {
        return Err(problem("\"minecraft\" no es una versión"));
    }

    Ok(Manifest {
        name,
        version,
        forge: parse_forge(&root, &minecraft)?,
        server: parse_server(&root)?,
        managed_folders: parse_managed_folders(&root)?,
        files: parse_files(&root)?,
        minecraft,
    })
}

fn parse_forge(root: &Value, minecraft: &str) -> Result<ForgeSpec, GameError> {
    let forge = root.get("forge").ok_or_else(|| problem("falta \"forge\""))?;
    let version = text(forge, "version")?.to_owned();
    if !is_version_number(&version) {
        return Err(problem("\"forge.version\" no es una versión"));
    }

    // El instalador solo se acepta del servidor oficial de Forge y con el
    // nombre exacto que corresponde a esta versión.
    let installer_url = text(forge, "installerUrl")?.to_owned();
    let expected_name = format!("/forge-{minecraft}-{version}-installer.jar");
    if https_host(&installer_url) != Some(FORGE_HOST) || !installer_url.ends_with(&expected_name) {
        return Err(problem("\"forge.installerUrl\" no es el instalador oficial"));
    }

    let installer_sha1 = text(forge, "installerSha1")?.to_ascii_lowercase();
    if !is_hex(&installer_sha1, 40) {
        return Err(problem("\"forge.installerSha1\" no es una huella SHA-1"));
    }

    Ok(ForgeSpec {
        version,
        installer_url,
        installer_sha1,
    })
}

fn parse_server(root: &Value) -> Result<ServerSpec, GameError> {
    let server = root.get("server").ok_or_else(|| problem("falta \"server\""))?;
    let address = text(server, "address")?.to_owned();
    if !is_host_name(&address) {
        return Err(problem("\"server.address\" no es una dirección"));
    }

    let port = match server.get("port") {
        None => 25565,
        Some(value) => value
            .as_u64()
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port != 0)
            .ok_or_else(|| problem("\"server.port\" no es un puerto"))?,
    };

    let lan_address = match server.get("lanAddress").and_then(Value::as_str) {
        None | Some("") => None,
        Some(lan) if is_host_name(lan) => Some(lan.to_owned()),
        Some(_) => return Err(problem("\"server.lanAddress\" no es una dirección")),
    };

    Ok(ServerSpec {
        address,
        port,
        lan_address,
    })
}

fn parse_managed_folders(root: &Value) -> Result<Vec<String>, GameError> {
    let Some(value) = root.get("managedFolders") else {
        return Ok(Vec::new());
    };
    let list = value
        .as_array()
        .ok_or_else(|| problem("\"managedFolders\" no es una lista"))?;

    let mut folders = Vec::new();
    for entry in list {
        match entry.as_str() {
            Some(folder) if MANAGEABLE.contains(&folder) => {
                if !folders.iter().any(|known| known == folder) {
                    folders.push(folder.to_owned());
                }
            }
            _ => return Err(problem("\"managedFolders\" solo admite mods, resourcepacks y shaderpacks")),
        }
    }
    Ok(folders)
}

fn parse_files(root: &Value) -> Result<Vec<ManifestFile>, GameError> {
    let list = root
        .get("files")
        .and_then(Value::as_array)
        .ok_or_else(|| problem("falta \"files\""))?;
    if list.len() > MAX_FILES {
        return Err(problem("demasiados archivos"));
    }

    let mut files = Vec::with_capacity(list.len());
    let mut seen = HashSet::new();
    for entry in list {
        let path = text(entry, "path")?.to_owned();
        let bad_path = || problem(&format!("ruta no permitida: {path}"));

        if !path.split('/').all(is_safe_component) || path.len() > 300 {
            return Err(bad_path());
        }
        let allowed = match path.split_once('/') {
            Some((folder, _)) => WRITABLE_FOLDERS.contains(&folder),
            None => WRITABLE_FILES.contains(&path.as_str()),
        };
        // ".part" es el nombre que usan las descargas a medias.
        if !allowed || path.to_lowercase().ends_with(".part") {
            return Err(bad_path());
        }
        // Windows no distingue mayúsculas: dos rutas iguales salvo por eso chocarían.
        if !seen.insert(path.to_lowercase()) {
            return Err(problem(&format!("ruta repetida: {path}")));
        }

        let size = entry
            .get("size")
            .and_then(Value::as_u64)
            .filter(|size| (1..=MAX_FILE_SIZE).contains(size))
            .ok_or_else(|| problem(&format!("tamaño no válido en {path}")))?;

        let sha512 = text(entry, "sha512")?.to_ascii_lowercase();
        if !is_hex(&sha512, 128) {
            return Err(problem(&format!("huella SHA-512 no válida en {path}")));
        }

        let url = text(entry, "url")?.to_owned();
        if check_url(&url).is_err() {
            return Err(problem(&format!("dirección no permitida en {path}")));
        }

        files.push(ManifestFile {
            path,
            size,
            sha512,
            url,
        });
    }

    // Ningún archivo puede llamarse igual que una carpeta que usa otro.
    for file in &files {
        let mut folder = file.path.to_lowercase();
        while let Some(cut) = folder.rfind('/') {
            folder.truncate(cut);
            if seen.contains(&folder) {
                return Err(problem(&format!("ruta repetida: {folder}")));
            }
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA512: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn sample(files: &str, extra: &str) -> String {
        format!(
            r#"{{
              "formatVersion": 1, "name": "Meca", "version": "1.0.0", "minecraft": "1.20.1",
              "forge": {{
                "version": "47.4.10",
                "installerUrl": "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.4.10/forge-1.20.1-47.4.10-installer.jar",
                "installerSha1": "66bfea9963bfa60d88bab6b2750e74a958392715"
              }},
              "server": {{ "address": "177.245.193.121", "port": 25565, "lanAddress": "192.168.100.73" }},
              "managedFolders": ["mods"],
              "files": [{files}]
              {extra}
            }}"#
        )
    }

    fn file(path: &str, url: &str) -> String {
        format!(r#"{{ "path": "{path}", "size": 341451, "sha1": "x", "sha512": "{SHA512}", "url": "{url}" }}"#)
    }

    const MOD_URL: &str = "https://cdn.modrinth.com/data/fALzjamp/versions/4FTDk9wv/Chunky-1.3.146.jar";

    #[test]
    fn ignores_the_mark_notepad_adds() {
        let mut data = b"\xEF\xBB\xBF".to_vec();
        data.extend_from_slice(sample("", "").as_bytes());
        assert_eq!(parse(&data).unwrap().name, "Meca");
    }

    #[test]
    fn knows_which_files_must_match_exactly() {
        let files = format!("{},{}", file("mods/a.jar", MOD_URL), file("config/a.toml", MOD_URL));
        let manifest = parse(sample(&files, "").as_bytes()).unwrap();
        assert!(manifest.is_managed("mods/a.jar"));
        assert!(!manifest.is_managed("config/a.toml"));
        assert!(!manifest.is_managed("options.txt"));
    }

    #[test]
    fn reads_a_valid_manifest() {
        let manifest = parse(sample(&file("mods/Chunky-1.3.146.jar", MOD_URL), "").as_bytes()).unwrap();
        assert_eq!(manifest.name, "Meca");
        assert_eq!(manifest.minecraft, "1.20.1");
        assert_eq!(manifest.forge.version, "47.4.10");
        assert_eq!(manifest.server.address, "177.245.193.121");
        assert_eq!(manifest.server.port, 25565);
        assert_eq!(manifest.server.lan_address.as_deref(), Some("192.168.100.73"));
        assert_eq!(manifest.managed_folders, ["mods"]);
        assert_eq!(manifest.files.len(), 1);
        assert_eq!(manifest.files[0].size, 341451);
    }

    fn rejects(json: String) -> &'static str {
        parse(json.as_bytes()).expect_err("debería rechazarse").code
    }

    #[test]
    fn rejects_dangerous_paths_and_untrusted_hosts() {
        for path in [
            "../evil.jar",
            "mods/../../evil.jar",
            "C:/evil.jar",
            "/evil.jar",
            "mods\\evil.jar",
            // Fuera de las carpetas permitidas: lo del jugador y lo del launcher.
            "saves/world/level.dat",
            "SCREEN~1/x.png",
            "Mods/x.jar",
            "modpack-version.txt",
            "evil.jar",
            "mods",
            "mods/x.jar.part",
        ] {
            assert_eq!(rejects(sample(&file(path, MOD_URL), "")), "bad_manifest", "{path}");
        }
        for path in ["mods/x.jar", "config/voicechat/voicechat-client.properties", "options.txt"] {
            assert!(parse(sample(&file(path, MOD_URL), "").as_bytes()).is_ok(), "{path}");
        }
        let clash = format!("{},{}", file("mods/a.jar", MOD_URL), file("mods/a.jar/b.jar", MOD_URL));
        assert_eq!(rejects(sample(&clash, "")), "bad_manifest");
        for url in ["http://cdn.modrinth.com/x.jar", "https://evil.example/x.jar", "https://cdn.modrinth.com@evil.example/x.jar"] {
            assert_eq!(rejects(sample(&file("mods/x.jar", url), "")), "bad_manifest", "{url}");
        }
        let twice = format!("{},{}", file("mods/A.jar", MOD_URL), file("mods/a.jar", MOD_URL));
        assert_eq!(rejects(sample(&twice, "")), "bad_manifest");
    }

    #[test]
    fn rejects_an_installer_that_is_not_the_official_one() {
        let good = sample("", "");
        assert!(parse(good.as_bytes()).is_ok());
        let other_host = good.replace("https://maven.minecraftforge.net/", "https://cdn.modrinth.com/");
        assert_eq!(rejects(other_host), "bad_manifest");
        let other_file = good.replace("forge-1.20.1-47.4.10-installer.jar", "forge-1.20.1-47.4.9-installer.jar");
        assert_eq!(rejects(other_file), "bad_manifest");
    }

    #[test]
    fn rejects_odd_servers_and_folders() {
        let good = sample("", "");
        assert_eq!(rejects(good.replace("177.245.193.121", "host --demo")), "bad_manifest");
        assert_eq!(rejects(good.replace("\"port\": 25565", "\"port\": 70000")), "bad_manifest");
        assert_eq!(rejects(good.replace("[\"mods\"]", "[\"saves\"]")), "bad_manifest");
        assert_eq!(rejects(good.replace("\"formatVersion\": 1", "\"formatVersion\": 2")), "outdated_launcher");
    }
}
