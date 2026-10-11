//! Guarda el refresh token en el Administrador de credenciales de Windows,
//! cifrado por el sistema para el usuario actual. Nunca se escribe en un
//! archivo ni llega a la interfaz.

use keyring::Entry;

use super::config::CREDENTIAL_SERVICE;
use super::error::AuthError;

/// Windows limita el tamaño de cada credencial, y el token puede superarlo,
/// así que se reparte en trozos de este número de caracteres.
const CHUNK_CHARS: usize = 1000;

const COUNT_KEY: &str = "refresh-count";

fn chunk_key(index: usize) -> String {
    format!("refresh-{index}")
}

fn entry(key: &str) -> Result<Entry, AuthError> {
    Entry::new(CREDENTIAL_SERVICE, key).map_err(storage_error)
}

fn storage_error(error: keyring::Error) -> AuthError {
    log::warn!("fallo al usar el Administrador de credenciales: {error}");
    AuthError::new(
        "storage",
        "No se pudo guardar tu sesión en Windows. Tendrás que iniciar sesión cada vez.",
    )
}

/// Parte el texto en trozos de `size` caracteres.
fn split_chunks(text: &str, size: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars
        .chunks(size)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect()
}

pub fn save_refresh_token(token: &str) -> Result<(), AuthError> {
    clear()?;

    let chunks = split_chunks(token, CHUNK_CHARS);
    for (index, chunk) in chunks.iter().enumerate() {
        entry(&chunk_key(index))?
            .set_password(chunk)
            .map_err(storage_error)?;
    }
    entry(COUNT_KEY)?
        .set_password(&chunks.len().to_string())
        .map_err(storage_error)
}

pub fn load_refresh_token() -> Result<Option<String>, AuthError> {
    let Some(count) = stored_count()? else {
        return Ok(None);
    };

    let mut token = String::new();
    for index in 0..count {
        match entry(&chunk_key(index))?.get_password() {
            Ok(chunk) => token.push_str(&chunk),
            // Falta un trozo: lo guardado no sirve.
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(error) => return Err(storage_error(error)),
        }
    }

    Ok(if token.is_empty() { None } else { Some(token) })
}

/// Borra la sesión guardada. No falla si no había nada.
pub fn clear() -> Result<(), AuthError> {
    if let Some(count) = stored_count()? {
        for index in 0..count {
            remove(&chunk_key(index))?;
        }
    }
    remove(COUNT_KEY)
}

fn stored_count() -> Result<Option<usize>, AuthError> {
    match entry(COUNT_KEY)?.get_password() {
        // Un número absurdo indicaría datos corruptos; se trata como "nada guardado".
        Ok(value) => Ok(value.parse::<usize>().ok().filter(|count| *count <= 64)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(storage_error(error)),
    }
}

fn remove(key: &str) -> Result<(), AuthError> {
    match entry(key)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(storage_error(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_long_tokens_and_keeps_every_character() {
        let token = "a".repeat(2500) + "ñ-fin";
        let chunks = split_chunks(&token, CHUNK_CHARS);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].chars().count(), 1000);
        assert_eq!(chunks.concat(), token);
        assert!(split_chunks("", CHUNK_CHARS).is_empty());
    }
}
