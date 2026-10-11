//! Errores de la instalación y el arranque, ya redactados para el jugador.

use std::io;

use serde::Serialize;

use crate::auth::AuthError;

/// Misma forma que `AuthError`: un código estable y un mensaje listo para mostrar.
#[derive(Debug, Clone, Serialize)]
pub struct GameError {
    pub code: &'static str,
    pub message: String,
}

impl GameError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn network() -> Self {
        Self::new(
            "network",
            "No se pudo descargar. Revisa tu conexión a internet e inténtalo de nuevo.",
        )
    }

    /// Un archivo no coincide con su huella aunque se intentó varias veces.
    pub fn corrupt() -> Self {
        Self::new(
            "corrupt",
            "Un archivo llegó dañado varias veces seguidas. Inténtalo de nuevo en unos minutos.",
        )
    }

    /// Fallo al leer o escribir en el disco. El detalle técnico va al registro.
    pub fn disk(context: &str, error: &io::Error) -> Self {
        log::warn!("{context}: {error}");
        if error.kind() == io::ErrorKind::StorageFull {
            return Self::new(
                "disk_full",
                "No queda espacio en el disco. Libera al menos 2 GB e inténtalo de nuevo.",
            );
        }
        Self::new(
            "disk",
            "No se pudo guardar un archivo del juego. Cierra Minecraft si está abierto e inténtalo de nuevo.",
        )
    }

    /// Un dato descargado no tiene la forma esperada. `what` es su nombre visible.
    pub fn invalid(what: &str) -> Self {
        log::warn!("formato inesperado: {what}");
        Self::new(
            "unexpected",
            format!("{what} tiene un formato que este launcher no reconoce. Puede que necesites una versión más nueva del launcher."),
        )
    }

    pub fn internal() -> Self {
        Self::new(
            "internal",
            "El launcher tuvo un problema interno. Ciérralo y vuelve a abrirlo.",
        )
    }
}

impl From<AuthError> for GameError {
    fn from(error: AuthError) -> Self {
        Self {
            code: error.code,
            message: error.message,
        }
    }
}
