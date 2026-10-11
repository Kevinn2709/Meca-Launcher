//! Errores del inicio de sesión, ya redactados para mostrarlos al jugador.

use serde::Serialize;

/// Lo que recibe la interfaz cuando algo falla: un código estable, por si
/// quiere reaccionar distinto a cada caso, y un mensaje listo para mostrar.
#[derive(Debug, Clone, Serialize)]
pub struct AuthError {
    pub code: &'static str,
    pub message: String,
}

impl AuthError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// El jugador canceló. La interfaz vuelve al inicio sin mostrar error.
    pub fn cancelled() -> Self {
        Self::new("cancelled", "Inicio de sesión cancelado.")
    }

    pub fn timed_out() -> Self {
        Self::new(
            "timeout",
            "Se acabó el tiempo para iniciar sesión. Inténtalo de nuevo.",
        )
    }

    pub fn network() -> Self {
        Self::new(
            "network",
            "No se pudo conectar. Revisa tu conexión a internet e inténtalo de nuevo.",
        )
    }

    /// Respuesta inesperada de un servicio. `service` es su nombre visible.
    pub fn unexpected(service: &str) -> Self {
        Self::new(
            "unexpected",
            format!("{service} respondió algo inesperado. Inténtalo de nuevo en unos minutos."),
        )
    }

    pub fn internal() -> Self {
        Self::new(
            "internal",
            "El launcher tuvo un problema interno. Ciérralo y vuelve a abrirlo.",
        )
    }
}

impl From<reqwest::Error> for AuthError {
    fn from(error: reqwest::Error) -> Self {
        // El detalle técnico va al registro; al jugador, un mensaje claro.
        log::warn!("fallo de red en el inicio de sesión: {error}");
        Self::network()
    }
}
