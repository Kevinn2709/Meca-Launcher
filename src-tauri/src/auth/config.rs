//! Datos fijos del inicio de sesión.

use std::time::Duration;

/// "Id. de aplicación (cliente)" del registro en Azure. No es un secreto:
/// identifica al launcher ante Microsoft y viaja en cada inicio de sesión.
pub const CLIENT_ID: &str = "020af44f-9252-4229-9178-04253249cdb6";

/// Solo cuentas personales ("consumers"): Xbox Live no acepta otras.
pub const AUTHORIZE_URL: &str =
    "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
pub const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";

/// `XboxLive.signin` da acceso a Xbox Live; `offline_access` permite recordar
/// la sesión para no pedir la contraseña cada vez que se abre el launcher.
pub const SCOPE: &str = "XboxLive.signin offline_access";

pub const XBOX_USER_URL: &str = "https://user.auth.xboxlive.com/user/authenticate";
pub const XBOX_XSTS_URL: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";

pub const MINECRAFT_LOGIN_URL: &str =
    "https://api.minecraftservices.com/authentication/login_with_xbox";
pub const MINECRAFT_ENTITLEMENTS_URL: &str =
    "https://api.minecraftservices.com/entitlements/mcstore";
pub const MINECRAFT_PROFILE_URL: &str = "https://api.minecraftservices.com/minecraft/profile";

/// Tiempo máximo para terminar el inicio de sesión en el navegador.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Tiempo máximo de cada petición a los servidores.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Vida mínima que debe quedarle al token de Minecraft al abrir el juego.
/// Con menos, se renueva antes: el juego tarda en cargar y el servidor
/// rechaza a quien llega con el token caducado.
pub const LAUNCH_MARGIN: Duration = Duration::from_secs(30 * 60);

/// Nombre con el que se guardan las credenciales en Windows.
/// Coincide con el `identifier` de tauri.conf.json.
pub const CREDENTIAL_SERVICE: &str = "com.meca.launcher";
