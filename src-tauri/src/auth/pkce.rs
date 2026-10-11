//! PKCE: la prueba que impide que otro programa canjee el código de inicio
//! de sesión que Microsoft devuelve al navegador.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use super::error::AuthError;

/// Texto aleatorio seguro, apto para ir en una URL.
/// `bytes` es la cantidad de bytes aleatorios; el texto resultante es más largo.
pub fn random_token(bytes: usize) -> Result<String, AuthError> {
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).map_err(|_| AuthError::internal())?;
    Ok(URL_SAFE_NO_PAD.encode(&buffer))
}

/// El "reto" que se envía a Microsoft: SHA-256 del verificador, en base64 para URL.
pub fn challenge_for(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_rfc_7636_example() {
        assert_eq!(
            challenge_for("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_tokens_are_long_and_different() {
        let first = random_token(64).unwrap();
        let second = random_token(64).unwrap();
        assert_eq!(first.len(), 86);
        assert_ne!(first, second);
        assert!(first.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }
}
