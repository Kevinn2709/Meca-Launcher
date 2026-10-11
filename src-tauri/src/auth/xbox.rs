//! Segundo tramo: Xbox Live. Con el token de Microsoft se obtiene un token
//! de usuario de Xbox y, con ese, un token XSTS válido para Minecraft.

use serde::Deserialize;
use serde_json::json;

use super::config::{XBOX_USER_URL, XBOX_XSTS_URL};
use super::error::AuthError;

/// Lo que Minecraft necesita de Xbox para identificar al jugador.
#[derive(Debug)]
pub struct XboxIdentity {
    pub xsts_token: String,
    pub user_hash: String,
}

#[derive(Debug, Deserialize)]
struct XboxToken {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    display_claims: DisplayClaims,
}

#[derive(Debug, Deserialize)]
struct DisplayClaims {
    xui: Vec<UserClaim>,
}

#[derive(Debug, Deserialize)]
struct UserClaim {
    uhs: String,
}

#[derive(Debug, Deserialize)]
struct XstsFailure {
    #[serde(rename = "XErr")]
    xerr: Option<u64>,
}

pub async fn authenticate(
    http: &reqwest::Client,
    microsoft_access_token: &str,
) -> Result<XboxIdentity, AuthError> {
    // 1. Token de usuario de Xbox Live.
    let response = http
        .post(XBOX_USER_URL)
        .header("Accept", "application/json")
        .json(&json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={microsoft_access_token}"),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))
        .send()
        .await?;

    if !response.status().is_success() {
        log::warn!("Xbox Live rechazó el token de Microsoft: {}", response.status());
        return Err(AuthError::unexpected("Xbox Live"));
    }
    let user: XboxToken = response
        .json()
        .await
        .map_err(|_| AuthError::unexpected("Xbox Live"))?;

    // 2. Token XSTS para los servicios de Minecraft.
    let response = http
        .post(XBOX_XSTS_URL)
        .header("Accept", "application/json")
        .json(&json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [user.token],
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let xerr = response
            .json::<XstsFailure>()
            .await
            .ok()
            .and_then(|failure| failure.xerr);
        log::warn!("Xbox Live (XSTS) respondió {status}, XErr {xerr:?}");
        return Err(xsts_error(xerr));
    }

    let xsts: XboxToken = response
        .json()
        .await
        .map_err(|_| AuthError::unexpected("Xbox Live"))?;
    let user_hash = xsts
        .display_claims
        .xui
        .into_iter()
        .next()
        .map(|claim| claim.uhs)
        .ok_or_else(|| AuthError::unexpected("Xbox Live"))?;

    Ok(XboxIdentity {
        xsts_token: xsts.token,
        user_hash,
    })
}

/// Traduce los códigos de error conocidos de Xbox a algo que el jugador pueda resolver.
fn xsts_error(xerr: Option<u64>) -> AuthError {
    match xerr {
        Some(2148916233) => AuthError::new(
            "no_xbox_account",
            "Esta cuenta de Microsoft no tiene perfil de Xbox. Créalo en xbox.com e inténtalo de nuevo.",
        ),
        Some(2148916235) => AuthError::new(
            "xbox_region",
            "Xbox Live no está disponible en el país de esta cuenta.",
        ),
        Some(2148916236) | Some(2148916237) => AuthError::new(
            "xbox_adult_check",
            "Xbox pide verificar la edad de esta cuenta. Hazlo en xbox.com e inténtalo de nuevo.",
        ),
        Some(2148916238) => AuthError::new(
            "xbox_child",
            "Es una cuenta de menor de edad. Un adulto debe añadirla a su grupo familiar de Microsoft.",
        ),
        Some(2148916227) => AuthError::new(
            "xbox_banned",
            "Esta cuenta está suspendida en Xbox Live.",
        ),
        _ => AuthError::unexpected("Xbox Live"),
    }
}
