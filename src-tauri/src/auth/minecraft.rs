//! Tercer tramo: servicios de Minecraft. Se obtiene el token del juego,
//! se comprueba que la cuenta lo tiene comprado y se lee el perfil.

use serde::Deserialize;
use serde_json::json;

use super::config::{MINECRAFT_ENTITLEMENTS_URL, MINECRAFT_LOGIN_URL, MINECRAFT_PROFILE_URL};
use super::error::AuthError;
use super::xbox::XboxIdentity;

#[derive(Debug, Deserialize)]
pub struct MinecraftLogin {
    pub access_token: String,
    /// Segundos de validez del token (normalmente 24 horas).
    pub expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct Entitlements {
    #[serde(default)]
    items: Vec<Entitlement>,
}

#[derive(Debug, Deserialize)]
struct Entitlement {
    name: String,
}

#[derive(Debug, Deserialize)]
pub struct Profile {
    /// UUID del jugador, sin guiones.
    pub id: String,
    pub name: String,
    #[serde(default)]
    skins: Vec<Skin>,
}

#[derive(Debug, Deserialize)]
struct Skin {
    state: String,
    url: String,
}

impl Profile {
    /// Textura de la skin activa, siempre por HTTPS.
    pub fn skin_url(&self) -> Option<String> {
        self.skins
            .iter()
            .find(|skin| skin.state == "ACTIVE")
            .or_else(|| self.skins.first())
            .map(|skin| skin.url.replacen("http://", "https://", 1))
    }
}

/// Cambia la identidad de Xbox por un token de Minecraft.
pub async fn login(
    http: &reqwest::Client,
    identity: &XboxIdentity,
) -> Result<MinecraftLogin, AuthError> {
    let response = http
        .post(MINECRAFT_LOGIN_URL)
        .header("Accept", "application/json")
        .json(&json!({
            "identityToken": format!("XBL3.0 x={};{}", identity.user_hash, identity.xsts_token),
        }))
        .send()
        .await?;

    let status = response.status();
    if status.is_success() {
        return response
            .json()
            .await
            .map_err(|_| AuthError::unexpected("Minecraft"));
    }

    log::warn!("Minecraft rechazó el inicio de sesión: {status}");
    if status.as_u16() == 403 {
        // Mojang solo acepta apps de Azure que haya aprobado (aka.ms/mce-reviewappid).
        return Err(AuthError::new(
            "not_approved",
            "Microsoft y Xbox aceptaron tu cuenta, pero Mojang aún no aprueba este launcher para usar la API de Minecraft.",
        ));
    }
    Err(AuthError::unexpected("Minecraft"))
}

/// Comprueba que la cuenta tiene Minecraft: Java Edition.
pub async fn ensure_owned(http: &reqwest::Client, access_token: &str) -> Result<(), AuthError> {
    let response = http
        .get(MINECRAFT_ENTITLEMENTS_URL)
        .bearer_auth(access_token)
        .send()
        .await?;

    if !response.status().is_success() {
        log::warn!("No se pudo consultar la propiedad del juego: {}", response.status());
        return Err(AuthError::unexpected("Minecraft"));
    }
    let entitlements: Entitlements = response
        .json()
        .await
        .map_err(|_| AuthError::unexpected("Minecraft"))?;

    let owns_java = entitlements
        .items
        .iter()
        .any(|item| item.name == "product_minecraft" || item.name == "game_minecraft");
    if owns_java {
        Ok(())
    } else {
        Err(AuthError::new(
            "not_owned",
            "Esta cuenta no tiene Minecraft: Java Edition. Usa la cuenta con la que compraste el juego.",
        ))
    }
}

/// Nombre, UUID y skin del jugador.
pub async fn profile(http: &reqwest::Client, access_token: &str) -> Result<Profile, AuthError> {
    let response = http
        .get(MINECRAFT_PROFILE_URL)
        .bearer_auth(access_token)
        .send()
        .await?;

    let status = response.status();
    if status.as_u16() == 404 {
        return Err(AuthError::new(
            "no_profile",
            "La cuenta aún no tiene nombre de jugador. Abre una vez el launcher oficial de Minecraft para crearlo.",
        ));
    }
    if !status.is_success() {
        log::warn!("No se pudo leer el perfil de Minecraft: {status}");
        return Err(AuthError::unexpected("Minecraft"));
    }

    response
        .json()
        .await
        .map_err(|_| AuthError::unexpected("Minecraft"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_with(skins: Vec<Skin>) -> Profile {
        Profile {
            id: "986dec87b7ec47ff89ff033fdb95c4b5".into(),
            name: "Jugador".into(),
            skins,
        }
    }

    #[test]
    fn picks_the_active_skin_over_https() {
        let profile = profile_with(vec![
            Skin { state: "INACTIVE".into(), url: "http://textures.minecraft.net/texture/old".into() },
            Skin { state: "ACTIVE".into(), url: "http://textures.minecraft.net/texture/current".into() },
        ]);
        assert_eq!(
            profile.skin_url().as_deref(),
            Some("https://textures.minecraft.net/texture/current")
        );
        assert_eq!(profile_with(vec![]).skin_url(), None);
    }
}
