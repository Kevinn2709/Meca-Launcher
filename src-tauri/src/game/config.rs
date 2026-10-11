//! Datos fijos de la instalación y el arranque del juego.

use std::time::Duration;

/// Lista de archivos del modpack. Vive en tu repositorio de GitHub:
/// al cambiar ese archivo, todos los launchers se actualizan solos.
pub const MANIFEST_URL: &str =
    "https://raw.githubusercontent.com/Kevinn2709/Meca-Launcher/main/manifest.json";

/// Índice oficial de versiones de Minecraft.
pub const VERSION_MANIFEST_URL: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";

/// Índice oficial de las versiones de Java que distribuye Mojang.
pub const JAVA_RUNTIMES_URL: &str = "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

/// Java que se usa si la versión del juego no indica otro (Java 17).
pub const DEFAULT_JAVA_COMPONENT: &str = "java-runtime-gamma";

/// Sonidos, idiomas y texturas del juego.
pub const ASSETS_URL: &str = "https://resources.download.minecraft.net";

/// Único servidor del que se acepta el instalador de Forge.
pub const FORGE_HOST: &str = "maven.minecraftforge.net";

/// Únicos servidores de los que el launcher acepta descargar algo.
/// Cualquier otra dirección se rechaza, venga de donde venga.
pub const ALLOWED_HOSTS: &[&str] = &[
    // Tu repositorio y, si algún día subes archivos propios, sus "releases".
    "raw.githubusercontent.com",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    // Mods.
    "cdn.modrinth.com",
    // Forge.
    "maven.minecraftforge.net",
    // Minecraft y Java (Mojang).
    "libraries.minecraft.net",
    "piston-meta.mojang.com",
    "piston-data.mojang.com",
    "launchermeta.mojang.com",
    "launcher.mojang.com",
    "resources.download.minecraft.net",
];

/// Descargas simultáneas.
pub const PARALLEL_DOWNLOADS: usize = 8;

/// Veces que se intenta cada archivo antes de darse por vencido. Con la
/// pausa entre intentos, aguanta un corte de wifi de unos diez segundos.
pub const DOWNLOAD_ATTEMPTS: u32 = 5;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Tiempo máximo sin recibir datos. No limita la descarga completa,
/// así que un archivo grande con conexión lenta no se corta.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Tamaño máximo de los archivos de datos (listas e índices).
pub const METADATA_LIMIT: u64 = 16 * 1024 * 1024;

/// Tamaño máximo de un archivo cuyo tamaño no se conoce de antemano.
pub const UNKNOWN_SIZE_LIMIT: u64 = 512 * 1024 * 1024;

/// Cada cuánto se vuelve a consultar la lista del modpack.
pub const MANIFEST_MAX_AGE: Duration = Duration::from_secs(60);

/// Tiempo máximo para que termine el instalador de Forge.
pub const FORGE_INSTALL_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// Espera máxima para conectar con el servidor por la red local y por internet.
/// En la red local la respuesta es inmediata o no llega: se espera poco.
pub const LAN_CONNECT_TIMEOUT: Duration = Duration::from_millis(800);
pub const PUBLIC_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Una vez conectado, espera máxima a que el servidor conteste.
pub const PING_REPLY_TIMEOUT: Duration = Duration::from_secs(3);

/// Si el juego se cierra con error antes de este tiempo, se da por hecho que
/// no llegó a arrancar bien y la próxima vez se revisan todos los archivos.
pub const EARLY_CRASH: Duration = Duration::from_secs(180);

/// Nombre con el que el launcher se presenta ante los servidores de descarga.
pub const USER_AGENT: &str = concat!(
    "MecaLauncher/",
    env!("CARGO_PKG_VERSION"),
    " (github.com/Kevinn2709/Meca-Launcher)"
);
