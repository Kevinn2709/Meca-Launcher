//! Primer tramo: cuenta de Microsoft (OAuth 2.0 con código de autorización y PKCE).

use serde::Deserialize;
use url::Url;

use super::config::{AUTHORIZE_URL, CLIENT_ID, SCOPE, TOKEN_URL};
use super::error::AuthError;

#[derive(Debug, Deserialize)]
pub struct MicrosoftTokens {
    pub access_token: String,
    /// Sirve para renovar la sesión sin volver a pedir la contraseña.
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MicrosoftFailure {
    error: String,
    error_description: Option<String>,
}

/// Dirección que se abre en el navegador para que el jugador inicie sesión.
pub fn authorize_url(redirect_uri: &str, state: &str, challenge: &str) -> Result<String, AuthError> {
    let url = Url::parse_with_params(
        AUTHORIZE_URL,
        &[
            ("client_id", CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", redirect_uri),
            ("response_mode", "query"),
            ("scope", SCOPE),
            ("state", state),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            // Deja elegir cuenta aunque el navegador ya tenga una abierta.
            ("prompt", "select_account"),
        ],
    )
    .map_err(|_| AuthError::internal())?;

    Ok(url.to_string())
}

/// Canjea el código que devolvió el navegador por los tokens de Microsoft.
pub async fn exchange_code(
    http: &reqwest::Client,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<MicrosoftTokens, AuthError> {
    request_tokens(
        http,
        &[
            ("client_id", CLIENT_ID),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
            ("scope", SCOPE),
        ],
    )
    .await
}

/// Renueva la sesión con el refresh token guardado.
pub async fn refresh(
    http: &reqwest::Client,
    refresh_token: &str,
) -> Result<MicrosoftTokens, AuthError> {
    request_tokens(
        http,
        &[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", SCOPE),
        ],
    )
    .await
}

async fn request_tokens(
    http: &reqwest::Client,
    form: &[(&str, &str)],
) -> Result<MicrosoftTokens, AuthError> {
    let response = http.post(TOKEN_URL).form(form).send().await?;
    let status = response.status();

    if status.is_success() {
        return response
            .json::<MicrosoftTokens>()
            .await
            .map_err(|_| AuthError::unexpected("Microsoft"));
    }

    let failure = response.json::<MicrosoftFailure>().await.ok();
    let code = failure.as_ref().map(|f| f.error.as_str()).unwrap_or("");
    log::warn!(
        "Microsoft rechazó la petición de tokens ({status}): {code} {}",
        failure
            .as_ref()
            .and_then(|f| f.error_description.as_deref())
            .unwrap_or("")
    );

    Err(match code {
        // El refresh token caducó o fue revocado: hay que iniciar sesión otra vez.
        "invalid_grant" => AuthError::new(
            "expired",
            "Tu sesión caducó. Inicia sesión de nuevo.",
        ),
        // Azure espera un secreto: la app no está registrada como de escritorio.
        "invalid_client" | "unauthorized_client" => AuthError::new(
            "azure_config",
            "Azure no trata la app como de escritorio. Mueve http://localhost a \"Aplicaciones móviles y de escritorio\".",
        ),
        _ => AuthError::unexpected("Microsoft"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_sign_in_address() {
        let address = authorize_url("http://localhost:53682", "abc", "challenge-value").unwrap();
        assert!(address.starts_with(
            "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize?client_id="
        ));
        assert!(address.contains("redirect_uri=http%3A%2F%2Flocalhost%3A53682"));
        assert!(address.contains("scope=XboxLive.signin+offline_access"));
        assert!(address.contains("code_challenge=challenge-value"));
        assert!(address.contains("code_challenge_method=S256"));
        assert!(address.contains("state=abc"));
    }
}
