//! Lectura de los archivos de versión de Minecraft y de Forge
//! (`versions/<id>/<id>.json`): librerías, argumentos y descargas.

use std::collections::HashSet;

use serde_json::Value;

use super::error::GameError;
use super::hash::is_hex;

/// Sistema para el que se preparan librerías y argumentos.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Environment {
    /// Como lo llama Mojang: "windows", "linux" u "osx".
    pub os: &'static str,
    /// "x86" solo en sistemas de 32 bits.
    pub arch: &'static str,
}

impl Environment {
    pub fn current() -> Self {
        Self {
            os: match std::env::consts::OS {
                "windows" => "windows",
                "macos" => "osx",
                _ => "linux",
            },
            arch: match std::env::consts::ARCH {
                "x86" => "x86",
                "aarch64" => "arm64",
                _ => "x64",
            },
        }
    }

    /// Separador entre las rutas del classpath de Java.
    pub fn classpath_separator(&self) -> &'static str {
        if self.os == "windows" { ";" } else { ":" }
    }
}

/// Un archivo remoto con lo necesario para comprobarlo.
#[derive(Debug, Clone, PartialEq)]
pub struct Remote {
    pub url: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    /// `grupo:artefacto[:clasificador]`, sin versión: identifica la librería
    /// para que la de Forge sustituya a la del juego si ambas la traen.
    pub key: String,
    /// Ruta dentro de `libraries/`, con `/`.
    pub path: String,
    /// `None` si la crea el instalador de Forge en vez de descargarse.
    pub download: Option<Remote>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetIndex {
    pub id: String,
    pub download: Remote,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Version {
    pub id: String,
    pub inherits_from: Option<String>,
    pub main_class: Option<String>,
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    pub libraries: Vec<Library>,
    pub asset_index: Option<AssetIndex>,
    pub client: Option<Remote>,
    pub java_component: Option<String>,
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// Lee un archivo remoto. `Ok(None)` si no trae dirección de descarga.
/// Una huella con forma incorrecta es un error: nunca se trata como
/// "sin huella", porque eso dejaría pasar el archivo sin comprobar.
fn remote(value: &Value) -> Result<Option<Remote>, GameError> {
    let Some(url) = text(value, "url").filter(|url| !url.is_empty()) else {
        return Ok(None);
    };
    let sha1 = match value.get("sha1") {
        None => None,
        Some(sha1) => Some(
            sha1.as_str()
                .filter(|sha1| is_hex(sha1, 40))
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| GameError::invalid("El archivo de versión del juego"))?,
        ),
    };
    Ok(Some(Remote {
        url: url.to_owned(),
        sha1,
        size: value.get("size").and_then(Value::as_u64),
    }))
}

/// Evalúa las reglas de una librería o de un argumento.
/// Sin reglas, se usa siempre. Con reglas, manda la última que aplique.
fn allowed(rules: Option<&Value>, env: &Environment, features: &[&str]) -> bool {
    let Some(rules) = rules.and_then(Value::as_array) else {
        return true;
    };
    let mut verdict = false;
    for rule in rules {
        if rule_applies(rule, env, features) {
            verdict = text(rule, "action") == Some("allow");
        }
    }
    verdict
}

fn rule_applies(rule: &Value, env: &Environment, features: &[&str]) -> bool {
    if let Some(os) = rule.get("os") {
        if text(os, "name").is_some_and(|name| name != env.os) {
            return false;
        }
        if text(os, "arch").is_some_and(|arch| arch != env.arch) {
            return false;
        }
        // Reglas por versión exacta del sistema: solo las usan juegos antiguos.
        if os.get("version").is_some() {
            return false;
        }
    }
    if let Some(wanted) = rule.get("features").and_then(Value::as_object) {
        for (feature, value) in wanted {
            let enabled = features.contains(&feature.as_str());
            if value.as_bool() != Some(enabled) {
                return false;
            }
        }
    }
    true
}

/// `grupo:artefacto:versión[:clasificador][@extensión]` -> (clave, ruta).
fn maven_coordinates(name: &str) -> Option<(String, String)> {
    let (name, extension) = name.split_once('@').unwrap_or((name, "jar"));
    let parts: Vec<&str> = name.split(':').collect();
    let (group, artifact, version, classifier) = match parts.as_slice() {
        [group, artifact, version] => (*group, *artifact, *version, None),
        [group, artifact, version, classifier] => (*group, *artifact, *version, Some(*classifier)),
        _ => return None,
    };
    if [group, artifact, version].iter().any(|part| part.is_empty()) {
        return None;
    }

    let file = match classifier {
        Some(classifier) => format!("{artifact}-{version}-{classifier}.{extension}"),
        None => format!("{artifact}-{version}.{extension}"),
    };
    let path = format!("{}/{artifact}/{version}/{file}", group.replace('.', "/"));
    let key = match classifier {
        Some(classifier) => format!("{group}:{artifact}:{classifier}"),
        None => format!("{group}:{artifact}"),
    };
    Some((key, path))
}

fn parse_library(value: &Value) -> Result<Library, GameError> {
    let name = text(value, "name").ok_or_else(|| GameError::invalid("Una librería del juego"))?;
    let (key, default_path) =
        maven_coordinates(name).ok_or_else(|| GameError::invalid("Una librería del juego"))?;

    let artifact = value.get("downloads").and_then(|downloads| downloads.get("artifact"));
    let path = artifact
        .and_then(|artifact| text(artifact, "path"))
        .map(str::to_owned)
        .unwrap_or(default_path);

    Ok(Library {
        key,
        path,
        download: match artifact {
            Some(artifact) => remote(artifact)?,
            None => None,
        },
    })
}

/// Aplana una lista de argumentos: textos sueltos y bloques con reglas.
fn parse_arguments(list: Option<&Value>, env: &Environment, features: &[&str]) -> Vec<String> {
    let mut arguments = Vec::new();
    for entry in list.and_then(Value::as_array).into_iter().flatten() {
        match entry {
            Value::String(argument) => arguments.push(argument.clone()),
            Value::Object(_) if allowed(entry.get("rules"), env, features) => {
                match entry.get("value") {
                    Some(Value::String(argument)) => arguments.push(argument.clone()),
                    Some(Value::Array(values)) => {
                        arguments.extend(values.iter().filter_map(Value::as_str).map(str::to_owned));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    arguments
}

/// Lee un archivo de versión. `features` son las opciones de arranque
/// activas (por ejemplo, entrar directo a un servidor).
pub fn parse(json: &Value, env: &Environment, features: &[&str]) -> Result<Version, GameError> {
    let id = text(json, "id")
        .ok_or_else(|| GameError::invalid("El archivo de versión del juego"))?
        .to_owned();

    // Formato de versiones anteriores a 1.13: este launcher no lo usa.
    if json.get("arguments").is_none() && json.get("minecraftArguments").is_some() {
        return Err(GameError::invalid("El archivo de versión del juego"));
    }
    let arguments = json.get("arguments");

    let mut libraries = Vec::new();
    for entry in json.get("libraries").and_then(Value::as_array).into_iter().flatten() {
        if allowed(entry.get("rules"), env, features) {
            libraries.push(parse_library(entry)?);
        }
    }

    let asset_index = match json.get("assetIndex") {
        Some(index) => match (text(index, "id"), remote(index)?) {
            (Some(id), Some(download)) => Some(AssetIndex {
                id: id.to_owned(),
                download,
            }),
            _ => None,
        },
        None => None,
    };
    let client = match json.get("downloads").and_then(|downloads| downloads.get("client")) {
        Some(client) => remote(client)?,
        None => None,
    };

    Ok(Version {
        id,
        inherits_from: text(json, "inheritsFrom").map(str::to_owned),
        main_class: text(json, "mainClass").map(str::to_owned),
        jvm_args: parse_arguments(arguments.and_then(|a| a.get("jvm")), env, features),
        game_args: parse_arguments(arguments.and_then(|a| a.get("game")), env, features),
        libraries,
        asset_index,
        client,
        java_component: json
            .get("javaVersion")
            .and_then(|java| text(java, "component"))
            .map(str::to_owned),
    })
}

/// Combina Forge (`child`) con la versión del juego de la que hereda (`parent`),
/// igual que el launcher oficial: argumentos del juego primero y luego los de
/// Forge; librerías de Forge delante y sin repetir las que comparten.
pub fn merge(child: Version, parent: Version) -> Version {
    let mut seen = HashSet::new();
    let libraries = child
        .libraries
        .into_iter()
        .chain(parent.libraries)
        .filter(|library| seen.insert(library.key.clone()))
        .collect();

    let mut jvm_args = parent.jvm_args;
    jvm_args.extend(child.jvm_args);
    let mut game_args = parent.game_args;
    game_args.extend(child.game_args);

    Version {
        id: child.id,
        inherits_from: Some(parent.id),
        main_class: child.main_class.or(parent.main_class),
        jvm_args,
        game_args,
        libraries,
        asset_index: child.asset_index.or(parent.asset_index),
        client: child.client.or(parent.client),
        java_component: child.java_component.or(parent.java_component),
    }
}

/// Valor que sigue a `flag` en una lista de argumentos (`--fml.mcpVersion 2023…`).
pub fn argument_value<'a>(arguments: &'a [String], flag: &str) -> Option<&'a str> {
    let position = arguments.iter().position(|argument| argument == flag)?;
    arguments.get(position + 1).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWS: Environment = Environment { os: "windows", arch: "x64" };
    const LINUX: Environment = Environment { os: "linux", arch: "x64" };

    /// Recorte fiel del archivo de versión de Minecraft 1.20.1.
    const VANILLA: &str = r#"{
      "id": "1.20.1", "mainClass": "net.minecraft.client.main.Main", "assets": "5", "type": "release",
      "javaVersion": {"component": "java-runtime-gamma", "majorVersion": 17},
      "assetIndex": {"id": "5", "sha1": "9d58fdd2538c6877fb5c5c558ebc60ee0b6d0e84", "size": 411581, "totalSize": 617718799,
                     "url": "https://piston-meta.mojang.com/v1/packages/9d58fdd2538c6877fb5c5c558ebc60ee0b6d0e84/5.json"},
      "downloads": {"client": {"sha1": "0c3ec587af28e5a785c0b4a7b8a30f9a8f78f838", "size": 23028853,
                               "url": "https://piston-data.mojang.com/v1/objects/0c3ec587af28e5a785c0b4a7b8a30f9a8f78f838/client.jar"}},
      "arguments": {
        "game": [
          "--username", "${auth_player_name}", "--version", "${version_name}",
          {"rules": [{"action": "allow", "features": {"is_demo_user": true}}], "value": "--demo"},
          {"rules": [{"action": "allow", "features": {"has_custom_resolution": true}}],
           "value": ["--width", "${resolution_width}", "--height", "${resolution_height}"]},
          {"rules": [{"action": "allow", "features": {"is_quick_play_multiplayer": true}}],
           "value": ["--quickPlayMultiplayer", "${quickPlayMultiplayer}"]}
        ],
        "jvm": [
          {"rules": [{"action": "allow", "os": {"name": "osx"}}], "value": ["-XstartOnFirstThread"]},
          {"rules": [{"action": "allow", "os": {"name": "windows"}}],
           "value": "-XX:HeapDumpPath=MojangTangentLauncher_ms_heapdump.dmp"},
          {"rules": [{"action": "allow", "os": {"arch": "x86"}}], "value": "-Xss1M"},
          "-Djava.library.path=${natives_directory}", "-cp", "${classpath}"
        ]
      },
      "libraries": [
        {"name": "com.mojang:logging:1.1.1",
         "downloads": {"artifact": {"path": "com/mojang/logging/1.1.1/logging-1.1.1.jar", "sha1": "832b8e6674a9b325a5175a3a6267dfaf34c85139",
                                    "size": 15343, "url": "https://libraries.minecraft.net/com/mojang/logging/1.1.1/logging-1.1.1.jar"}}},
        {"name": "org.lwjgl:lwjgl-glfw:3.3.1:natives-windows",
         "downloads": {"artifact": {"path": "org/lwjgl/lwjgl-glfw/3.3.1/lwjgl-glfw-3.3.1-natives-windows.jar", "sha1": "ed892f945cf7e79c8756796f32d00fa4ceaf573b",
                                    "size": 145512, "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl-glfw/3.3.1/lwjgl-glfw-3.3.1-natives-windows.jar"}},
         "rules": [{"action": "allow", "os": {"name": "windows"}}]},
        {"name": "org.lwjgl:lwjgl-glfw:3.3.1:natives-linux",
         "downloads": {"artifact": {"path": "org/lwjgl/lwjgl-glfw/3.3.1/lwjgl-glfw-3.3.1-natives-linux.jar", "sha1": "0766bb0e8e829598b1c8052fd8173c62af741c52",
                                    "size": 110704, "url": "https://libraries.minecraft.net/org/lwjgl/lwjgl-glfw/3.3.1/lwjgl-glfw-3.3.1-natives-linux.jar"}},
         "rules": [{"action": "allow", "os": {"name": "linux"}}]},
        {"name": "org.ow2.asm:asm:9.3",
         "downloads": {"artifact": {"path": "org/ow2/asm/asm/9.3/asm-9.3.jar", "sha1": "8e6300ef51c1d801a7ed62d07cd221aca3a90640",
                                    "size": 122004, "url": "https://libraries.minecraft.net/org/ow2/asm/asm/9.3/asm-9.3.jar"}}}
      ]
    }"#;

    fn vanilla(env: &Environment, features: &[&str]) -> Version {
        parse(&serde_json::from_str::<Value>(VANILLA).unwrap(), env, features).unwrap()
    }

    #[test]
    fn reads_downloads_and_java() {
        let version = vanilla(&WINDOWS, &[]);
        assert_eq!(version.id, "1.20.1");
        assert_eq!(version.java_component.as_deref(), Some("java-runtime-gamma"));
        assert_eq!(version.asset_index.as_ref().unwrap().id, "5");
        let client = version.client.unwrap();
        assert_eq!(client.size, Some(23028853));
        assert_eq!(client.sha1.as_deref(), Some("0c3ec587af28e5a785c0b4a7b8a30f9a8f78f838"));
    }

    #[test]
    fn libraries_follow_the_operating_system() {
        let paths = |env: &Environment| -> Vec<String> { vanilla(env, &[]).libraries.into_iter().map(|l| l.path).collect() };
        let windows = paths(&WINDOWS);
        assert!(windows.iter().any(|p| p.ends_with("natives-windows.jar")));
        assert!(!windows.iter().any(|p| p.ends_with("natives-linux.jar")));
        let linux = paths(&LINUX);
        assert!(linux.iter().any(|p| p.ends_with("natives-linux.jar")));
        assert!(!linux.iter().any(|p| p.ends_with("natives-windows.jar")));
    }

    #[test]
    fn arguments_follow_rules_and_features() {
        let plain = vanilla(&WINDOWS, &[]);
        assert_eq!(plain.game_args, ["--username", "${auth_player_name}", "--version", "${version_name}"]);
        assert_eq!(
            plain.jvm_args,
            [
                "-XX:HeapDumpPath=MojangTangentLauncher_ms_heapdump.dmp",
                "-Djava.library.path=${natives_directory}",
                "-cp",
                "${classpath}"
            ]
        );
        assert_eq!(vanilla(&LINUX, &[]).jvm_args[0], "-Djava.library.path=${natives_directory}");

        let direct = vanilla(&WINDOWS, &["is_quick_play_multiplayer"]);
        assert_eq!(&direct.game_args[4..], ["--quickPlayMultiplayer", "${quickPlayMultiplayer}"]);
        assert!(!direct.game_args.iter().any(|a| a == "--demo" || a == "--width"));
    }

    #[test]
    fn a_malformed_hash_is_an_error_not_a_missing_hash() {
        let broken = VANILLA.replace("832b8e6674a9b325a5175a3a6267dfaf34c85139", "no-es-una-huella");
        assert!(parse(&serde_json::from_str::<Value>(&broken).unwrap(), &WINDOWS, &[]).is_err());
        let broken = VANILLA.replace("0c3ec587af28e5a785c0b4a7b8a30f9a8f78f838\", \"size", "zz\", \"size");
        assert!(parse(&serde_json::from_str::<Value>(&broken).unwrap(), &WINDOWS, &[]).is_err());
    }

    #[test]
    fn maven_names_become_paths() {
        assert_eq!(
            maven_coordinates("net.minecraftforge:mergetool:1.1.5:api"),
            Some((
                "net.minecraftforge:mergetool:api".into(),
                "net/minecraftforge/mergetool/1.1.5/mergetool-1.1.5-api.jar".into()
            ))
        );
        assert_eq!(
            maven_coordinates("de.oceanlabs.mcp:mcp_config:1.20.1-20230612.114412@zip").unwrap().1,
            "de/oceanlabs/mcp/mcp_config/1.20.1-20230612.114412/mcp_config-1.20.1-20230612.114412.zip"
        );
        assert_eq!(maven_coordinates("no-es-una-librería"), None);
    }

    #[test]
    fn forge_goes_on_top_of_the_game() {
        let forge = parse(
            &serde_json::from_str::<Value>(
                r#"{
                  "id": "1.20.1-forge-47.4.10", "inheritsFrom": "1.20.1",
                  "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
                  "arguments": {"game": ["--launchTarget", "forgeclient", "--fml.mcpVersion", "20230612.114412"],
                                "jvm": ["-DlibraryDirectory=${library_directory}"]},
                  "libraries": [
                    {"name": "org.ow2.asm:asm:9.8",
                     "downloads": {"artifact": {"path": "org/ow2/asm/asm/9.8/asm-9.8.jar", "sha1": "dc19ecb3f7889b7860697215cae99c0f9b6f6b4b",
                                                "size": 126113, "url": "https://maven.minecraftforge.net/org/ow2/asm/asm/9.8/asm-9.8.jar"}}}
                  ]
                }"#,
            )
            .unwrap(),
            &WINDOWS,
            &[],
        )
        .unwrap();
        assert_eq!(argument_value(&forge.game_args, "--fml.mcpVersion"), Some("20230612.114412"));

        let merged = merge(forge, vanilla(&WINDOWS, &[]));
        assert_eq!(merged.id, "1.20.1-forge-47.4.10");
        assert_eq!(merged.inherits_from.as_deref(), Some("1.20.1"));
        assert_eq!(merged.main_class.as_deref(), Some("cpw.mods.bootstraplauncher.BootstrapLauncher"));
        // La librería de Forge va primero y sustituye a la misma del juego.
        assert_eq!(merged.libraries[0].path, "org/ow2/asm/asm/9.8/asm-9.8.jar");
        assert!(!merged.libraries.iter().any(|l| l.path.ends_with("asm-9.3.jar")));
        // Argumentos: primero los del juego, luego los de Forge.
        assert_eq!(merged.jvm_args.last().unwrap(), "-DlibraryDirectory=${library_directory}");
        assert_eq!(merged.game_args[0], "--username");
        assert_eq!(merged.game_args.last().unwrap(), "20230612.114412");
        assert!(merged.client.is_some() && merged.asset_index.is_some());
    }
}
