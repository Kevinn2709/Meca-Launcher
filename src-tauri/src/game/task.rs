//! Trabajo que bloquea (disco, procesos, esperas) fuera de los hilos de red.

use std::time::Duration;

use super::error::GameError;

/// Ejecuta `work` en un hilo aparte y espera su resultado.
pub async fn blocking<T, F>(work: F) -> Result<T, GameError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| {
            log::warn!("una tarea interna no terminó: {error}");
            GameError::internal()
        })
}

pub async fn pause(duration: Duration) {
    let _ = blocking(move || std::thread::sleep(duration)).await;
}
