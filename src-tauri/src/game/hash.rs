//! Huellas SHA-1 y SHA-512 para comprobar que cada archivo es el esperado.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha1::Sha1;
use sha2::{Digest, Sha512};

/// Huella que debe tener un archivo, en hexadecimal y minúsculas.
/// Mojang y Forge publican SHA-1; la lista del modpack usa SHA-512.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expected {
    Sha1(String),
    Sha512(String),
}

impl Expected {
    fn value(&self) -> &str {
        match self {
            Expected::Sha1(value) | Expected::Sha512(value) => value,
        }
    }

    pub fn matches(&self, actual: &str) -> bool {
        self.value().eq_ignore_ascii_case(actual)
    }
}

/// Calcula la huella a medida que llegan los datos.
pub enum Hasher {
    Sha1(Sha1),
    Sha512(Sha512),
}

impl Hasher {
    pub fn matching(expected: &Expected) -> Self {
        match expected {
            Expected::Sha1(_) => Hasher::Sha1(Sha1::new()),
            Expected::Sha512(_) => Hasher::Sha512(Sha512::new()),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Sha1(hasher) => hasher.update(data),
            Hasher::Sha512(hasher) => hasher.update(data),
        }
    }

    pub fn finish(self) -> String {
        match self {
            Hasher::Sha1(hasher) => hex(hasher.finalize().as_slice()),
            Hasher::Sha512(hasher) => hex(hasher.finalize().as_slice()),
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// `true` si `text` son exactamente `length` dígitos hexadecimales.
pub fn is_hex(text: &str, length: usize) -> bool {
    text.len() == length && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub fn bytes_match(data: &[u8], expected: &Expected) -> bool {
    let mut hasher = Hasher::matching(expected);
    hasher.update(data);
    expected.matches(&hasher.finish())
}

/// Lee el archivo completo y dice si su huella es la esperada.
pub fn file_matches(path: &Path, expected: &Expected) -> io::Result<bool> {
    let mut file = File::open(path)?;
    let mut hasher = Hasher::matching(expected);
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(expected.matches(&hasher.finish()))
}

/// `true` si el archivo está en disco tal como se publicó.
///
/// Lo normal es comparar solo el tamaño, que es inmediato. Con `deep`
/// además se calcula la huella: es lo que repara una instalación que se
/// interrumpió a medias (por ejemplo, por un corte de luz).
pub fn is_intact(path: &Path, size: Option<u64>, expected: Option<&Expected>, deep: bool) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() || size.is_some_and(|size| size != metadata.len()) {
        return false;
    }
    match expected {
        Some(expected) if deep => file_matches(path, expected).unwrap_or(false),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_hashes_of_abc() {
        let sha1 = Expected::Sha1("a9993e364706816aba3e25717850c26c9cd0d89d".into());
        assert!(bytes_match(b"abc", &sha1));
        assert!(!bytes_match(b"abd", &sha1));

        let sha512 = Expected::Sha512(
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
                .into(),
        );
        assert!(bytes_match(b"abc", &sha512));
        assert!(Expected::Sha1("A9993E364706816ABA3E25717850C26C9CD0D89D".into())
            .matches("a9993e364706816aba3e25717850c26c9cd0d89d"));
    }

    #[test]
    fn recognises_hex_strings() {
        assert!(is_hex("66bfea9963bfa60d88bab6b2750e74a958392715", 40));
        assert!(!is_hex("66bfea9963bfa60d88bab6b2750e74a95839271", 40));
        assert!(!is_hex("zzbfea9963bfa60d88bab6b2750e74a958392715", 40));
    }
}
