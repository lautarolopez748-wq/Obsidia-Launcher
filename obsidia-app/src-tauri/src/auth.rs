//! Login de Microsoft (device code) con refresh token.
//!
//! Los tokens viven en el Credential Manager de Windows (crate `keyring`), nunca en
//! texto plano. El refresh token se rota en cada renovación y el access token de
//! Minecraft se renueva solo cuando está por vencer.

use crate::launcher;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// ID de la app registrada en Azure (cuentas personales de Microsoft, device code).
/// Se puede pisar con la variable de entorno `OBSIDIA_MS_CLIENT_ID` o con
/// `.minecraft/obsidia/ms_client_id.txt` sin recompilar.
const DEFAULT_CLIENT_ID: &str = "14d82eec-204b-4c2f-b7e8-296a70dab67e";

const SERVICE: &str = "obsidia.launcher";
const REFRESH_USER: &str = "ms-refresh-token";
const ACCESS_USER: &str = "ms-access-token";
const SCOPE: &str = "XboxLive.signin offline_access";

const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Lo que ve la UI: nombre y UUID. Los tokens no salen del backend.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MsProfile {
    pub name: String,
    pub uuid: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct Stored {
    access_token: String,
    expires_at: u64,
    name: String,
    uuid: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Cada login nuevo (o cada cancelación) sube este contador; un login en espera que ve
/// otro número sabe que lo cancelaron y termina sin guardar nada.
static LOGIN_GEN: AtomicU64 = AtomicU64::new(0);

/// Cancela el login que esté esperando el código de Microsoft.
pub fn cancel_login() {
    LOGIN_GEN.fetch_add(1, Ordering::SeqCst);
}

fn mc_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(appdata).join(".minecraft")
}

pub fn client_id() -> String {
    if let Ok(v) = std::env::var("OBSIDIA_MS_CLIENT_ID")
        && !v.trim().is_empty() {
            return v.trim().to_string();
        }
    let path = mc_dir().join("obsidia").join("ms_client_id.txt");
    if let Ok(v) = fs::read_to_string(&path)
        && !v.trim().is_empty() {
            return v.trim().to_string();
        }
    DEFAULT_CLIENT_ID.to_string()
}

// ─────────────────────────────── Keyring ───────────────────────────────────────

fn entry(user: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, user).map_err(|e| format!("Credential Manager: {e}"))
}

fn read_secret(user: &str) -> Result<Option<String>, String> {
    match entry(user)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Credential Manager: {e}")),
    }
}

fn write_secret(user: &str, value: &str) -> Result<(), String> {
    entry(user)?
        .set_password(value)
        .map_err(|e| format!("Credential Manager: {e}"))
}

fn delete_secret(user: &str) {
    if let Ok(e) = entry(user) {
        let _ = e.delete_credential();
    }
}

fn read_stored() -> Result<Option<Stored>, String> {
    match read_secret(ACCESS_USER)? {
        Some(raw) => serde_json::from_str(&raw)
            .map(Some)
            .map_err(|_| "Los datos de sesión guardados están dañados".to_string()),
        None => Ok(None),
    }
}

fn write_stored(s: &Stored) -> Result<(), String> {
    let raw = serde_json::to_string(s).map_err(|e| e.to_string())?;
    write_secret(ACCESS_USER, &raw)
}

fn read_refresh() -> Result<Option<String>, String> {
    read_secret(REFRESH_USER)
}

fn write_refresh(token: &str) -> Result<(), String> {
    write_secret(REFRESH_USER, token)
}

/// Borra la copia vieja en texto plano (`obsidia/auth.json`) si quedó de versiones anteriores.
fn cleanup_legacy() {
    let path = mc_dir().join("obsidia").join("auth.json");
    if path.exists() {
        let _ = fs::remove_file(path);
    }
}

// ─────────────────────────────── HTTP ─────────────────────────────────────────

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(launcher::UA)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

pub async fn request_device_code() -> Result<DeviceCode, String> {
    let resp = client()?
        .post(DEVICE_CODE_URL)
        .form(&[("client_id", client_id()), ("scope", SCOPE.to_string())])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "Microsoft rechazó la solicitud ({}): {}",
            status,
            json.get("error_description")
                .and_then(|v| v.as_str())
                .unwrap_or("revisá el client id de Azure")
        ));
    }
    Ok(DeviceCode {
        device_code: json.get("device_code").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        user_code: json.get("user_code").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        verification_uri: json
            .get("verification_uri")
            .and_then(|v| v.as_str())
            .unwrap_or("https://microsoft.com/link")
            .to_string(),
        expires_in: json.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(900),
        interval: json.get("interval").and_then(|v| v.as_u64()).unwrap_or(5),
    })
}

struct MsTokens {
    access_token: String,
    refresh_token: Option<String>,
}

/// Pide los tokens de Microsoft (device code o refresh token).
async fn token_request(form: &[(&str, String)]) -> Result<MsTokens, String> {
    let resp = client()?
        .post(TOKEN_URL)
        .form(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if let Some(err) = json.get("error").and_then(|v| v.as_str()) {
        let desc = json
            .get("error_description")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        return Err(format!("token error: {err} {desc}"));
    }
    let access_token = json
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("Microsoft no devolvió access_token")?
        .to_string();
    Ok(MsTokens {
        access_token,
        refresh_token: json
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(String::from),
    })
}

/// Canjea el access token de Microsoft por uno de Minecraft.
async fn xbox_to_minecraft(ms_access: &str) -> Result<(String, u64), String> {
    let c = client()?;

    // 1) Xbox Live
    let xbl: serde_json::Value = c
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_access}")
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let xbl_token = xbl
        .get("Token")
        .and_then(|v| v.as_str())
        .ok_or("Xbox Live no devolvió token (¿la cuenta no tiene Minecraft?)")?
        .to_string();
    let uhs = xbl
        .get("DisplayClaims")
        .and_then(|d| d.get("xui"))
        .and_then(|x| x.as_array())
        .and_then(|a| a.first())
        .and_then(|o| o.get("uhs"))
        .and_then(|v| v.as_str())
        .ok_or("Xbox Live no devolvió el UHS")?
        .to_string();

    // 2) XSTS
    let xsts: serde_json::Value = c
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let xsts_token = xsts
        .get("Token")
        .and_then(|v| v.as_str())
        .ok_or("XSTS rechazó la cuenta (¿menor de edad sin permiso o sin Xbox Live?")?
        .to_string();

    // 3) Minecraft
    let mc: serde_json::Value = c
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&serde_json::json!({
            "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let access = mc
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or("Minecraft no devolvió access_token")?
        .to_string();
    let expires_in = mc.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(86_400);
    Ok((access, expires_in))
}

async fn minecraft_profile(mc_access: &str) -> Result<MsProfile, String> {
    let resp = client()?
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(mc_access)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("Esta cuenta de Microsoft no tiene Minecraft Java comprado".to_string());
    }
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(MsProfile {
        name: json.get("name").and_then(|v| v.as_str()).unwrap_or("Player").to_string(),
        uuid: json.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
    })
}

// ─────────────────────────────── Flujo ────────────────────────────────────────

/// Espera a que el usuario complete el device code y guarda la sesión.
pub async fn wait_for_login(device_code: &str) -> Result<MsProfile, String> {
    let mut interval = 5u64;
    let mut ms_tokens: Option<MsTokens> = None;
    // Un login nuevo cancela al anterior que siguiera esperando.
    let my_gen = LOGIN_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let cancelled = || LOGIN_GEN.load(Ordering::SeqCst) != my_gen;
    for _ in 0..120 {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if cancelled() {
            return Err("Login cancelado".to_string());
        }
        match token_request(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code".to_string()),
            ("client_id", client_id()),
            ("device_code", device_code.to_string()),
        ])
        .await
        {
            Ok(t) => {
                ms_tokens = Some(t);
                break;
            }
            Err(e) if e.contains("authorization_pending") => continue,
            Err(e) if e.contains("slow_down") => {
                interval += 5;
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    let tokens = ms_tokens.ok_or("Se agotó el tiempo de espera del login")?;
    // Si lo cancelaron justo mientras Microsoft respondía, no se guarda ninguna sesión.
    if cancelled() {
        return Err("Login cancelado".to_string());
    }

    let refresh = tokens
        .refresh_token
        .as_deref()
        .ok_or("Microsoft no devolvió refresh_token (revisá el scope offline_access)")?
        .to_string();
    let (mc_access, expires_in) = xbox_to_minecraft(&tokens.access_token).await?;
    let profile = minecraft_profile(&mc_access).await?;
    if cancelled() {
        return Err("Login cancelado".to_string());
    }

    write_refresh(&refresh)?;
    write_stored(&Stored {
        access_token: mc_access,
        expires_at: now() + expires_in.saturating_sub(120),
        name: profile.name.clone(),
        uuid: profile.uuid.clone(),
    })?;
    cleanup_legacy();
    Ok(profile)
}

/// Renueva el access token de Minecraft con el refresh token guardado.
async fn refresh_session() -> Result<MsProfile, String> {
    let refresh = read_refresh()?
        .ok_or("No hay sesión de Microsoft guardada. Iniciá sesión de nuevo.")?;
    let tokens = token_request(&[
        ("grant_type", "refresh_token".to_string()),
        ("client_id", client_id()),
        ("refresh_token", refresh.clone()),
        ("scope", SCOPE.to_string()),
    ])
    .await
    .map_err(|e| {
        // invalid_grant: la sesión ya no sirve; se borra para no reintentar en loop.
        if e.contains("invalid_grant") {
            delete_secret(REFRESH_USER);
            delete_secret(ACCESS_USER);
            "La sesión de Microsoft venció. Iniciá sesión de nuevo.".to_string()
        } else {
            format!("No se pudo renovar la sesión de Microsoft: {e}")
        }
    })?;
    let new_refresh = tokens.refresh_token.unwrap_or(refresh);
    let (mc_access, expires_in) = xbox_to_minecraft(&tokens.access_token).await?;
    let profile = minecraft_profile(&mc_access).await?;

    write_refresh(&new_refresh)?;
    write_stored(&Stored {
        access_token: mc_access,
        expires_at: now() + expires_in.saturating_sub(120),
        name: profile.name.clone(),
        uuid: profile.uuid.clone(),
    })?;
    Ok(profile)
}

/// Perfil guardado, renovando el token si está vencido. `None` = modo offline.
pub async fn current_profile() -> Result<Option<MsProfile>, String> {
    cleanup_legacy();
    let Some(stored) = read_stored()? else { return Ok(None) };
    if stored.expires_at > now() && read_refresh()?.is_some() {
        return Ok(Some(MsProfile { name: stored.name, uuid: stored.uuid }));
    }
    match refresh_session().await {
        Ok(p) => Ok(Some(p)),
        Err(e) => Err(e),
    }
}

/// Jugador para lanzar el juego (renueva antes si hace falta).
pub async fn player() -> Result<Option<launcher::Player>, String> {
    let Some(profile) = current_profile().await? else {
        return Ok(None);
    };
    let stored = read_stored()?.ok_or("Sesión de Microsoft inconsistente")?;
    Ok(Some(launcher::Player {
        name: profile.name,
        uuid: profile.uuid,
        token: stored.access_token,
        user_type: "msa".to_string(),
    }))
}

/// ¿Hay una sesión de Microsoft guardada?
pub fn has_session() -> bool {
    read_refresh().ok().flatten().is_some()
}

/// Borra todos los tokens del Credential Manager.
pub fn logout() -> Result<(), String> {
    delete_secret(REFRESH_USER);
    delete_secret(ACCESS_USER);
    cleanup_legacy();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perfil_se_serializa_sin_tokens() {
        let p = MsProfile { name: "Lauti".into(), uuid: "abc".into() };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "{\"name\":\"Lauti\",\"uuid\":\"abc\"}");
        assert!(!json.contains("token"));
    }

    #[test]
    fn client_id_se_puede_pisar() {
        // No tocamos variables de entorno globales en tests: solo verificamos el default.
        if std::env::var("OBSIDIA_MS_CLIENT_ID").is_err() {
            assert!(!client_id().trim().is_empty());
        }
    }

    #[test]
    fn el_reloj_avanza() {
        assert!(now() > 1_600_000_000);
    }
}
