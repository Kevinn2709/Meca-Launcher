//! Inicio de sesión con cuenta de Microsoft.
//!
//! Recorrido completo:
//!   1. El navegador del sistema abre la página de Microsoft (PKCE).
//!   2. Microsoft devuelve un código a un servidor local de un solo uso.
//!   3. Código -> token de Microsoft -> Xbox Live -> XSTS -> token de Minecraft.
//!   4. Se comprueba que la cuenta tiene el juego y se lee su perfil.
//!
//! La interfaz solo recibe nombre, UUID y skin. Los tokens se quedan aquí:
//! el de Minecraft en memoria y el de renovación en el Administrador de
//! credenciales de Windows.

mod config;
mod error;
mod loopback;
mod microsoft;
mod minecraft;
mod pkce;
mod store;
mod xbox;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

pub use error::AuthError;
use loopback::{Loopback, WaitError};
use microsoft::MicrosoftTokens;

/// Lo que la interfaz sabe del jugador. Coincide con `Session` en types.ts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub name: String,
    pub uuid: String,
    pub skin_url: Option<String>,
}

/// Sesión de juego vigente. No sale de Rust: solo la usa el arranque de
/// Minecraft, que pasa el token directamente al juego.
#[derive(Debug, Clone)]
pub struct GameSession {
    pub access_token: String,
    pub uuid: String,
    pub name: String,
    pub expires_at: Instant,
}

/// Estado compartido del inicio de sesión.
pub struct AuthState {
    http: reqwest::Client,
    game: Mutex<Option<GameSession>>,
    /// Se pone en `true` para abandonar un inicio de sesión en curso.
    cancel: Arc<AtomicBool>,
}

impl AuthState {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("MecaLauncher/", env!("CARGO_PKG_VERSION")))
            .timeout(config::REQUEST_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            http,
            game: Mutex::new(None),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Sesión de juego actual, si hay una y le queda al menos `margin` de vida.
    fn game_session(&self, margin: Duration) -> Option<GameSession> {
        let guard = self.game.lock().unwrap_or_else(PoisonError::into_inner);
        guard
            .as_ref()
            .filter(|session| session.expires_at > Instant::now() + margin)
            .cloned()
    }

    /// Sesión de juego lista para abrir Minecraft. El token dura 24 horas:
    /// si al de memoria le queda poco, se renueva con la sesión guardada en
    /// Windows. Así el juego nunca recibe un token que caduque mientras carga.
    pub async fn fresh_game_session(&self) -> Result<GameSession, AuthError> {
        if let Some(session) = self.game_session(config::LAUNCH_MARGIN) {
            return Ok(session);
        }

        let expired = || {
            AuthError::new(
                "expired",
                "Tu sesión caducó. Inicia sesión de nuevo para jugar.",
            )
        };
        let refresh_token = store::load_refresh_token()
            .ok()
            .flatten()
            .ok_or_else(expired)?;
        let tokens = microsoft::refresh(&self.http, &refresh_token).await?;
        complete_sign_in(self, tokens).await?;
        self.game_session(Duration::ZERO).ok_or_else(AuthError::internal)
    }

    fn set_game_session(&self, session: Option<GameSession>) {
        *self.game.lock().unwrap_or_else(PoisonError::into_inner) = session;
    }
}

/// Tramos 2 a 4: de los tokens de Microsoft a una sesión completa.
async fn complete_sign_in(
    state: &AuthState,
    tokens: MicrosoftTokens,
) -> Result<Session, AuthError> {
    let identity = xbox::authenticate(&state.http, &tokens.access_token).await?;
    let login = minecraft::login(&state.http, &identity).await?;
    minecraft::ensure_owned(&state.http, &login.access_token).await?;
    let profile = minecraft::profile(&state.http, &login.access_token).await?;

    // Solo se recuerda la sesión cuando todo el recorrido salió bien.
    if let Some(refresh_token) = tokens.refresh_token.as_deref() {
        if let Err(error) = store::save_refresh_token(refresh_token) {
            log::warn!("la sesión no se pudo guardar: {}", error.message);
        }
    }

    let session = Session {
        name: profile.name.clone(),
        uuid: profile.id.clone(),
        skin_url: profile.skin_url(),
    };
    state.set_game_session(Some(GameSession {
        access_token: login.access_token,
        uuid: profile.id,
        name: profile.name,
        // Un minuto de margen para no usar un token a punto de caducar.
        expires_at: Instant::now() + Duration::from_secs(login.expires_in.saturating_sub(60)),
    }));

    log::info!("sesión iniciada como {}", session.name);
    Ok(session)
}

/// Al abrir el launcher: recupera la sesión guardada sin pedir contraseña.
/// Devuelve `None` si no hay sesión o si ya no sirve.
#[tauri::command]
pub async fn restore_session(state: State<'_, AuthState>) -> Result<Option<Session>, AuthError> {
    let refresh_token = match store::load_refresh_token() {
        Ok(Some(token)) => token,
        Ok(None) | Err(_) => return Ok(None),
    };

    let tokens = match microsoft::refresh(&state.http, &refresh_token).await {
        Ok(tokens) => tokens,
        Err(error) => {
            if error.code == "expired" {
                let _ = store::clear();
            }
            return Ok(None);
        }
    };

    match complete_sign_in(&state, tokens).await {
        Ok(session) => Ok(Some(session)),
        Err(error) => {
            log::warn!("no se pudo recuperar la sesión: {}", error.message);
            Ok(None)
        }
    }
}

/// Inicio de sesión interactivo: abre el navegador y espera a que el jugador termine.
#[tauri::command]
pub async fn sign_in(app: AppHandle, state: State<'_, AuthState>) -> Result<Session, AuthError> {
    state.cancel.store(false, Ordering::SeqCst);

    let loopback = Loopback::bind().map_err(|error| {
        log::warn!("no se pudo abrir el puerto local: {error}");
        AuthError::internal()
    })?;
    let redirect_uri = loopback.redirect_uri();
    let verifier = pkce::random_token(64)?;
    let expected_state = pkce::random_token(24)?;
    let address = microsoft::authorize_url(
        &redirect_uri,
        &expected_state,
        &pkce::challenge_for(&verifier),
    )?;

    open::that(&address).map_err(|error| {
        log::warn!("no se pudo abrir el navegador: {error}");
        AuthError::new(
            "browser",
            "No se pudo abrir tu navegador. Comprueba que tienes uno predeterminado en Windows.",
        )
    })?;

    // La espera bloquea, así que va en un hilo aparte del de la interfaz.
    let cancel = state.cancel.clone();
    let waited = tauri::async_runtime::spawn_blocking(move || {
        loopback.wait_for_callback(&cancel, config::SIGN_IN_TIMEOUT)
    })
    .await
    .map_err(|_| AuthError::internal())?;

    let callback = match waited {
        Ok(callback) => callback,
        Err(WaitError::Cancelled) => return Err(AuthError::cancelled()),
        Err(WaitError::TimedOut) => return Err(AuthError::timed_out()),
    };

    // El jugador ya terminó en el navegador: se trae el launcher al frente.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }

    if let Some(error) = callback.error.as_deref() {
        log::warn!(
            "Microsoft devolvió un error: {error} {}",
            callback.error_description.as_deref().unwrap_or("")
        );
        // "access_denied" es el jugador pulsando Cancelar o No en Microsoft.
        return Err(if error == "access_denied" {
            AuthError::cancelled()
        } else {
            AuthError::unexpected("Microsoft")
        });
    }

    // El `state` debe ser el que enviamos: descarta respuestas que no pedimos.
    if callback.state.as_deref() != Some(expected_state.as_str()) {
        log::warn!("la respuesta de Microsoft no corresponde a este inicio de sesión");
        return Err(AuthError::unexpected("Microsoft"));
    }
    let code = callback
        .code
        .ok_or_else(|| AuthError::unexpected("Microsoft"))?;

    let tokens = microsoft::exchange_code(&state.http, &code, &verifier, &redirect_uri).await?;
    complete_sign_in(&state, tokens).await
}

/// Abandona el inicio de sesión que está esperando al navegador.
#[tauri::command]
pub fn cancel_sign_in(state: State<'_, AuthState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// Cierra la sesión y olvida lo guardado en Windows.
#[tauri::command]
pub fn sign_out(state: State<'_, AuthState>) -> Result<(), AuthError> {
    state.set_game_session(None);
    store::clear()
}
