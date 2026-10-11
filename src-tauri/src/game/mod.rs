//! Instalación, actualización y arranque del juego (Paso 4).
//!
//! Recorrido de "Jugar":
//!   1. Se lee la lista del modpack de tu repositorio de GitHub.
//!   2. Se comprueba qué falta de Java 17, Minecraft, Forge y los mods.
//!   3. Se descarga solo eso, comprobando la huella de cada archivo.
//!   4. La primera vez, se instala Forge con su instalador oficial.
//!   5. Se abre Minecraft con los flags de Aikar y entra directo al servidor.

mod config;
mod download;
mod error;
mod events;
mod forge;
mod hash;
mod java;
mod launch;
mod manifest;
mod paths;
mod ping;
mod pipeline;
mod sync;
mod task;
mod vanilla;
mod version;

use std::fs::{self, File, TryLockError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::auth::AuthState;
pub use error::GameError;
use events::{Events, PlayEvent};
use launch::Player;
use manifest::Manifest;
use paths::Dirs;

/// Estado del servidor. Coincide con `ServerStatus` en types.ts.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    online: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    players: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_players: Option<u64>,
}

/// Estado del modpack en este equipo. Coincide con `ModpackStatus` en types.ts.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackStatus {
    installed: Option<String>,
    latest: String,
    pending_files: u64,
    pending_bytes: u64,
    /// `false` hasta que Java, Minecraft y Forge estén instalados.
    game_installed: bool,
}

/// Estado compartido de la instalación y el arranque.
pub struct GameState {
    http: reqwest::Client,
    /// `true` mientras se instala o se juega: evita dos tareas a la vez.
    busy: AtomicBool,
    /// Última lista del modpack leída, para no pedirla en cada consulta.
    manifest: Mutex<Option<(Instant, Arc<Manifest>)>>,
}

/// Marca el launcher como ocupado y lo libera al terminar, pase lo que pase.
struct Busy<'a> {
    flag: &'a AtomicBool,
    /// Candado en disco: avisa a otras ventanas del launcher. Se suelta
    /// solo al cerrarse el archivo, también si el launcher se cierra de golpe.
    _lock: Option<File>,
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}

impl GameState {
    pub fn new() -> Self {
        Self {
            http: download::client(),
            busy: AtomicBool::new(false),
            manifest: Mutex::new(None),
        }
    }

    /// Reserva el turno para instalar o jugar. Solo puede haber una tarea
    /// a la vez, ni en esta ventana ni en otra copia abierta del launcher:
    /// dos instalaciones sobre la misma carpeta se estropearían entre sí.
    fn begin(&self, dirs: &Dirs) -> Result<Busy<'_>, GameError> {
        if self.busy.swap(true, Ordering::SeqCst) {
            return Err(GameError::new(
                "busy",
                "El launcher ya está preparando o ejecutando el juego.",
            ));
        }
        let mut busy = Busy {
            flag: &self.busy,
            _lock: None,
        };

        let cache = dirs.cache();
        let lock = fs::create_dir_all(&cache).and_then(|()| {
            File::options()
                .create(true)
                .write(true)
                .truncate(false)
                .open(cache.join("en-uso.lock"))
        });
        match lock {
            Ok(file) => match file.try_lock() {
                Ok(()) => busy._lock = Some(file),
                Err(TryLockError::WouldBlock) => {
                    return Err(GameError::new(
                        "busy",
                        "Hay otra ventana del launcher instalando o ejecutando el juego. Ciérrala o espera a que termine.",
                    ));
                }
                // Hay discos que no admiten candados: se sigue sin él.
                Err(TryLockError::Error(error)) => {
                    log::warn!("no se pudo tomar el candado del launcher: {error}");
                }
            },
            Err(error) => return Err(GameError::disk("no se pudo preparar la carpeta del launcher", &error)),
        }
        Ok(busy)
    }

    fn cached_manifest(&self) -> Option<Arc<Manifest>> {
        let guard = self.manifest.lock().unwrap_or_else(PoisonError::into_inner);
        guard
            .as_ref()
            .filter(|(read_at, _)| read_at.elapsed() < config::MANIFEST_MAX_AGE)
            .map(|(_, manifest)| manifest.clone())
    }

    /// Lista del modpack. Si no hay conexión, sirve la última que se guardó:
    /// así se puede seguir jugando aunque GitHub no responda.
    async fn manifest(&self, dirs: &Dirs) -> Result<Arc<Manifest>, GameError> {
        if let Some(manifest) = self.cached_manifest() {
            return Ok(manifest);
        }

        let saved = dirs.cache().join("manifest.json");
        let manifest = match download::fetch(&self.http, config::MANIFEST_URL, None).await {
            Ok(data) => {
                let manifest = manifest::parse(&data)?;
                let _ = fs::create_dir_all(dirs.cache());
                let _ = fs::write(&saved, &data);
                manifest
            }
            Err(error) => {
                let Ok(data) = fs::read(&saved) else {
                    return Err(error);
                };
                log::warn!("no se pudo leer la lista del modpack; se usa la guardada");
                manifest::parse(&data)?
            }
        };

        let manifest = Arc::new(manifest);
        *self.manifest.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((Instant::now(), manifest.clone()));
        Ok(manifest)
    }
}

fn dirs(app: &AppHandle) -> Result<Dirs, GameError> {
    let root = app.path().app_local_data_dir().map_err(|error| {
        log::warn!("no se encontró la carpeta de datos del launcher: {error}");
        GameError::internal()
    })?;
    Ok(Dirs::new(root))
}

fn events_to(channel: Channel<PlayEvent>) -> Arc<Events> {
    Arc::new(Events::new(move |event| {
        // Si la ventana se cerró, ya no hay a quién avisar.
        let _ = channel.send(event);
    }))
}

/// ¿Está encendido el servidor y cuántos jugadores hay?
#[tauri::command]
pub async fn server_status(
    app: AppHandle,
    state: State<'_, GameState>,
) -> Result<ServerStatus, GameError> {
    let manifest = state.manifest(&dirs(&app)?).await?;
    let server = manifest.server.clone();
    let found = task::blocking(move || ping::locate(&server)).await?;
    Ok(match found {
        Some((_, pong)) => ServerStatus {
            online: true,
            players: Some(pong.players),
            max_players: Some(pong.max_players),
        },
        None => ServerStatus {
            online: false,
            players: None,
            max_players: None,
        },
    })
}

/// Compara los mods de este equipo con la lista publicada.
#[tauri::command]
pub async fn modpack_status(
    app: AppHandle,
    state: State<'_, GameState>,
) -> Result<ModpackStatus, GameError> {
    let dirs = dirs(&app)?;
    let manifest = state.manifest(&dirs).await?;

    let instance = dirs.instance();
    let plan = {
        let manifest = manifest.clone();
        task::blocking(move || sync::plan(&instance, &manifest, None)).await??
    };
    Ok(ModpackStatus {
        installed: sync::installed_version(&dirs.instance()),
        latest: manifest.version.clone(),
        pending_files: plan.pending(),
        pending_bytes: plan.bytes,
        game_installed: pipeline::is_game_installed(&dirs, &manifest),
    })
}

/// Instala o actualiza el juego sin abrirlo. No necesita sesión iniciada.
#[tauri::command]
pub async fn prepare(
    app: AppHandle,
    state: State<'_, GameState>,
    on_event: Channel<PlayEvent>,
) -> Result<(), GameError> {
    let dirs = dirs(&app)?;
    let _busy = state.begin(&dirs)?;
    let events = events_to(on_event);
    let manifest = state.manifest(&dirs).await?;
    pipeline::prepare(&state.http, &dirs, &manifest, &events).await?;
    Ok(())
}

/// "Jugar": deja el juego al día, lo abre y espera a que se cierre.
#[tauri::command]
pub async fn play(
    app: AppHandle,
    auth: State<'_, AuthState>,
    state: State<'_, GameState>,
    on_event: Channel<PlayEvent>,
) -> Result<(), GameError> {
    let dirs = dirs(&app)?;
    let _busy = state.begin(&dirs)?;
    let events = events_to(on_event);

    // Sin una sesión válida no se descarga ni se abre nada.
    auth.fresh_game_session().await?;

    let manifest = state.manifest(&dirs).await?;
    let ready = pipeline::prepare(&state.http, &dirs, &manifest, &events).await?;

    // Preparar el juego puede tardar: la sesión se vuelve a pedir ahora
    // para que el juego no reciba un token a punto de caducar.
    let session = auth.fresh_game_session().await?;

    let launcher_version = app.package_info().version.to_string();
    let player = Player {
        name: &session.name,
        uuid: &session.uuid,
        access_token: &session.access_token,
    };
    pipeline::launch(&dirs, &manifest, &ready, player, &launcher_version, &events).await
}
