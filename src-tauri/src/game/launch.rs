//! Arranque de Minecraft: memoria, flags de Aikar, argumentos del juego y
//! de Forge, y conexión directa al servidor.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use super::error::GameError;
use super::paths::{Dirs, safe_join};
use super::version::{Environment, Version};

/// Datos del jugador que necesita el juego. El token solo vive en memoria.
pub struct Player<'a> {
    pub name: &'a str,
    pub uuid: &'a str,
    pub access_token: &'a str,
}

pub struct Launch<'a> {
    pub version: &'a Version,
    pub dirs: &'a Dirs,
    pub env: Environment,
    pub player: Player<'a>,
    /// Servidor al que entrar directamente: "dirección:puerto".
    pub server: Option<&'a str>,
    pub memory: Memory,
    pub launcher_version: &'a str,
}

/// Memoria que se le da al juego.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Memory {
    /// Tope de memoria del juego, en MB.
    pub max_mb: u64,
    /// Memoria reservada desde el principio, en MB.
    pub initial_mb: u64,
    /// Reservar y recorrer toda la memoria al arrancar (`AlwaysPreTouch`).
    pub pretouch: bool,
}

/// Memoria del equipo en MB, si el sistema la informa.
#[cfg(windows)]
fn total_memory_mb() -> Option<u64> {
    #[repr(C)]
    #[allow(dead_code)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }

    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // SAFETY: `status` es una estructura válida, del tamaño que anuncia en
    // `length`, y Windows solo escribe dentro de ella.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    (ok != 0).then(|| status.total_phys / (1024 * 1024))
}

#[cfg(not(windows))]
fn total_memory_mb() -> Option<u64> {
    let info = fs::read_to_string("/proc/meminfo").ok()?;
    let line = info.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb / 1024)
}

/// Memoria para el juego según la del equipo. Un modpack con Cataclysm va
/// justo con menos de 4 GB; más de 8 GB no aporta y alarga las pausas.
///
/// Aikar pensó sus flags para servidores, donde toda la memoria es para
/// Minecraft: reserva el tope entero desde el arranque. En un PC de juego
/// con 16 GB o más se hace igual. Con menos, eso dejaría sin memoria a
/// Windows y al navegador, así que se empieza con la mitad y crece si hace falta.
fn memory_for(total_mb: Option<u64>) -> Memory {
    let max_mb = match total_mb {
        None => 4096,
        Some(total) if total >= 23_000 => 8192,
        Some(total) if total >= 15_000 => 6144,
        Some(total) if total >= 11_000 => 5120,
        Some(total) if total >= 7_000 => 4096,
        Some(total) if total >= 5_000 => 3072,
        Some(_) => 2048,
    };
    let roomy = total_mb.is_some_and(|total| total >= 15_000);
    Memory {
        max_mb,
        initial_mb: if roomy { max_mb } else { max_mb / 2 },
        pretouch: roomy,
    }
}

pub fn memory() -> Memory {
    memory_for(total_memory_mb())
}

/// Flags de Aikar para Java 17 (la variante para menos de 12 GB de memoria).
/// Ajustan el recolector G1 para pausas cortas y regulares.
fn memory_arguments(memory: Memory) -> Vec<String> {
    let mut arguments = vec![
        format!("-Xms{}M", memory.initial_mb),
        format!("-Xmx{}M", memory.max_mb),
    ];
    arguments.extend(
        [
            "-XX:+UseG1GC",
            "-XX:+ParallelRefProcEnabled",
            "-XX:MaxGCPauseMillis=200",
            "-XX:+UnlockExperimentalVMOptions",
            "-XX:+DisableExplicitGC",
            "-XX:G1NewSizePercent=30",
            "-XX:G1MaxNewSizePercent=40",
            "-XX:G1HeapRegionSize=8M",
            "-XX:G1ReservePercent=20",
            "-XX:G1HeapWastePercent=5",
            "-XX:G1MixedGCCountTarget=4",
            "-XX:InitiatingHeapOccupancyPercent=15",
            "-XX:G1MixedGCLiveThresholdPercent=90",
            "-XX:G1RSetUpdatingPauseTimePercent=5",
            "-XX:SurvivorRatio=32",
            "-XX:+PerfDisableSharedMem",
            "-XX:MaxTenuringThreshold=1",
            "-Dusing.aikars.flags=https://mcflags.emc.gs",
            "-Daikars.new.flags=true",
        ]
        .map(String::from),
    );
    if memory.pretouch {
        arguments.push("-XX:+AlwaysPreTouch".to_owned());
    }
    arguments
}

/// Sustituye cada `${nombre}` por su valor, en una sola pasada: un valor
/// nunca se vuelve a interpretar. Los nombres desconocidos se dejan igual.
fn substitute(template: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                match values.iter().find(|(key, _)| *key == name) {
                    Some((_, value)) => out.push_str(value),
                    None => out.push_str(&rest[start..start + 2 + end + 1]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

/// Lista completa de argumentos para Java, en el orden del launcher oficial:
/// memoria, opciones de Java, clase principal y argumentos del juego.
pub fn arguments(launch: &Launch) -> Result<Vec<String>, GameError> {
    let version = launch.version;
    let dirs = launch.dirs;
    let invalid = || GameError::invalid("El archivo de versión del juego");
    let main_class = version.main_class.as_deref().ok_or_else(invalid)?;
    let asset_index = version.asset_index.as_ref().ok_or_else(invalid)?;

    let separator = launch.env.classpath_separator();
    let libraries = dirs.libraries();
    let mut classpath = Vec::with_capacity(version.libraries.len() + 1);
    for library in &version.libraries {
        classpath.push(text(&safe_join(&libraries, &library.path).ok_or_else(invalid)?));
    }
    // Como el launcher oficial: el cliente va al final con el nombre de la
    // versión de Forge, que es justo el que Forge sabe que debe ignorar.
    classpath.push(text(&dirs.version_jar(&version.id)));
    let classpath = classpath.join(separator);

    let natives = text(&dirs.natives(&version.id));
    let library_directory = text(&libraries);
    let game_directory = text(&dirs.instance());
    let assets_root = text(&dirs.assets());
    let values: [(&str, &str); 19] = [
        ("auth_player_name", launch.player.name),
        ("auth_uuid", launch.player.uuid),
        ("auth_access_token", launch.player.access_token),
        ("auth_xuid", ""),
        ("clientid", ""),
        ("user_type", "msa"),
        ("version_name", &version.id),
        ("version_type", "release"),
        ("game_directory", &game_directory),
        ("assets_root", &assets_root),
        ("assets_index_name", &asset_index.id),
        ("natives_directory", &natives),
        ("library_directory", &library_directory),
        ("classpath_separator", separator),
        ("classpath", &classpath),
        ("launcher_name", "meca-launcher"),
        ("launcher_version", launch.launcher_version),
        ("quickPlayMultiplayer", launch.server.unwrap_or("")),
        ("quickPlayPath", ""),
    ];

    let mut out = memory_arguments(launch.memory);
    out.extend(version.jvm_args.iter().map(|argument| substitute(argument, &values)));
    out.push(main_class.to_owned());

    let mut game = version.game_args.iter().map(|argument| substitute(argument, &values));
    while let Some(argument) = game.next() {
        // Opciones que quedarían con el valor vacío: se omiten enteras.
        let optional = matches!(argument.as_str(), "--clientId" | "--xuid" | "--quickPlayMultiplayer" | "--quickPlayPath");
        if optional {
            match game.next() {
                Some(value) if !value.is_empty() => out.extend([argument, value]),
                _ => {}
            }
        } else {
            out.push(argument);
        }
    }
    Ok(out)
}

/// Quita variables del sistema con las que otros programas cambian las
/// opciones de Java sin avisar (por ejemplo, limitando la memoria).
pub fn clean_environment(command: &mut Command) {
    for variable in ["_JAVA_OPTIONS", "JAVA_TOOL_OPTIONS", "JDK_JAVA_OPTIONS", "JAVA_OPTIONS"] {
        command.env_remove(variable);
    }
}

/// Evita que Windows abra una ventana negra de consola junto al proceso.
#[cfg(windows)]
pub fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console(_command: &mut Command) {}

/// Deja el cliente de Minecraft junto a la versión de Forge, con su nombre.
/// Es el mismo archivo: se enlaza y, si el disco no lo permite, se copia.
pub fn place_client(dirs: &Dirs, version: &Version) -> Result<(), GameError> {
    let Some(parent) = version.inherits_from.as_deref() else {
        return Ok(());
    };
    let source = dirs.version_jar(parent);
    let dest = dirs.version_jar(&version.id);
    let size = |path: &Path| fs::metadata(path).ok().map(|metadata| metadata.len());
    if size(&source).is_some() && size(&source) == size(&dest) {
        return Ok(());
    }

    let _ = fs::remove_file(&dest);
    if fs::hard_link(&source, &dest).is_err() {
        fs::copy(&source, &dest)
            .map_err(|error| GameError::disk("no se pudo preparar el cliente del juego", &error))?;
    }
    Ok(())
}

/// Abre el juego. La salida de Java se guarda en `log` por si no arranca.
pub fn spawn(java: &Path, arguments: &[String], dirs: &Dirs, version: &Version, log: &Path) -> Result<Child, GameError> {
    let disk = |context: &'static str| move |error: std::io::Error| GameError::disk(context, &error);
    let instance = dirs.instance();
    fs::create_dir_all(&instance).map_err(disk("no se pudo crear la carpeta del juego"))?;
    fs::create_dir_all(dirs.natives(&version.id)).map_err(disk("no se pudo crear la carpeta del juego"))?;
    if let Some(parent) = log.parent() {
        fs::create_dir_all(parent).map_err(disk("no se pudo crear la carpeta de trabajo"))?;
    }
    let output = File::create(log).map_err(disk("no se pudo crear el registro del juego"))?;
    let errors = output.try_clone().map_err(disk("no se pudo crear el registro del juego"))?;

    let mut command = Command::new(java);
    command
        .args(arguments)
        .current_dir(&instance)
        .stdin(Stdio::null())
        .stdout(Stdio::from(output))
        .stderr(Stdio::from(errors));
    clean_environment(&mut command);
    hide_console(&mut command);

    command.spawn().map_err(|error| {
        log::warn!("no se pudo abrir Java: {error}");
        GameError::new("java", "No se pudo abrir Java. Pulsa Reintentar para repararlo.")
    })
}

pub fn game_log(dirs: &Dirs) -> PathBuf {
    dirs.cache().join("minecraft-output.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_replaced_once() {
        let values = [("a", "1"), ("b", "${a}"), ("dir", "C:\\juego")];
        assert_eq!(substitute("-Dx=${a}", &values), "-Dx=1");
        assert_eq!(substitute("${dir}/libs${a}${a}", &values), "C:\\juego/libs11");
        // Un valor que parece otra variable no se vuelve a sustituir.
        assert_eq!(substitute("${b}", &values), "${a}");
        // Lo desconocido o a medias se queda como está.
        assert_eq!(substitute("${nope} ${a", &values), "${nope} ${a");
        assert_eq!(substitute("sin variables", &values), "sin variables");
    }

    #[test]
    fn memory_follows_the_machine() {
        assert_eq!(memory_for(None).max_mb, 4096);
        assert_eq!(memory_for(Some(3_900)).max_mb, 2048);
        assert_eq!(memory_for(Some(32_000)).max_mb, 8192);
        assert!(total_memory_mb().is_some());

        // 8 GB: la mitad al empezar, sin reservar todo de golpe.
        let modest = memory_for(Some(7_900));
        assert_eq!(modest, Memory { max_mb: 4096, initial_mb: 2048, pretouch: false });
        let flags = memory_arguments(modest);
        assert_eq!(&flags[..3], ["-Xms2048M", "-Xmx4096M", "-XX:+UseG1GC"]);
        assert!(!flags.contains(&"-XX:+AlwaysPreTouch".to_owned()));
        assert!(flags.contains(&"-XX:G1NewSizePercent=30".to_owned()));
        assert!(flags.contains(&"-Daikars.new.flags=true".to_owned()));

        // 16 GB: los flags de Aikar al completo.
        let roomy = memory_for(Some(15_800));
        assert_eq!(roomy, Memory { max_mb: 6144, initial_mb: 6144, pretouch: true });
        let flags = memory_arguments(roomy);
        assert_eq!(&flags[..3], ["-Xms6144M", "-Xmx6144M", "-XX:+UseG1GC"]);
        assert!(flags.contains(&"-XX:+AlwaysPreTouch".to_owned()));
    }
}
