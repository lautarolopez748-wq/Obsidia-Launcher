use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

fn new_cmd<S: AsRef<std::ffi::OsStr>>(program: S) -> Command {
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    c.creation_flags(0x08000000);
    c
}

fn minecraft_dir() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(appdata).join(".minecraft")
}

fn profiles_path() -> PathBuf {
    minecraft_dir().join("obsidia").join("profiles.json")
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Profile {
    pub name: String,
    pub version: String,
    pub username: String,
    pub ram: String,
    pub java_path: Option<String>,
}

#[derive(Serialize)]
pub struct JavaInfo {
    pub path: String,
    pub version: String,
}

#[tauri::command]
fn get_installed_versions() -> Result<Vec<String>, String> {
    let dir = minecraft_dir().join("versions");
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

#[tauri::command]
async fn list_versions() -> Result<Vec<String>, String> {
    let resp = reqwest::get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut ids = Vec::new();
    if let Some(versions) = json.get("versions").and_then(|v| v.as_array()) {
        for v in versions {
            if v.get("type").and_then(|t| t.as_str()) == Some("release") {
                if let Some(id) = v.get("id").and_then(|i| i.as_str()) {
                    ids.push(id.to_string());
                    if ids.len() >= 60 {
                        break;
                    }
                }
            }
        }
    }
    Ok(ids)
}

#[tauri::command]
fn get_profiles() -> Result<Vec<Profile>, String> {
    let path = profiles_path();
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let profiles: Vec<Profile> = serde_json::from_str(&data).unwrap_or_default();
    Ok(profiles)
}

#[tauri::command]
fn save_profile(profile: Profile) -> Result<(), String> {
    let path = profiles_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut profiles = get_profiles().unwrap_or_default();
    if let Some(existing) = profiles.iter_mut().find(|p| p.name == profile.name) {
        *existing = profile;
    } else {
        profiles.push(profile);
    }
    let data = serde_json::to_string_pretty(&profiles).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn delete_profile(name: String) -> Result<(), String> {
    let path = profiles_path();
    let mut profiles = get_profiles().unwrap_or_default();
    profiles.retain(|p| p.name != name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(&profiles).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(())
}

fn java_version(path: &Path) -> String {
    if let Ok(out) = new_cmd(path).arg("-version").output() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let text = if stderr.is_empty() { stdout } else { stderr };
        for line in text.lines() {
            if line.contains("version") {
                return line.trim().to_string();
            }
        }
        text.lines().next().unwrap_or("").trim().to_string()
    } else {
        String::new()
    }
}

#[tauri::command]
fn detect_javas() -> Result<Vec<JavaInfo>, String> {
    let mut found: Vec<PathBuf> = Vec::new();
    // known minecraft runtimes
    let mc = minecraft_dir().join("runtime");
    if mc.exists() {
        if let Ok(entries) = fs::read_dir(&mc) {
            for e in entries.flatten() {
                let bin = e.path().join("windows-x64").join(e.file_name()).join("bin").join("java.exe");
                // generic fallback search
                let alt = e.path().join("bin").join("java.exe");
                if bin.exists() { found.push(bin); }
                else if alt.exists() { found.push(alt); }
            }
        }
    }
    // common program files
    let candidates = [
        "C:\\Program Files\\Eclipse Adoptium",
        "C:\\Program Files\\Java",
        "C:\\Program Files (x86)\\Java",
        "C:\\Program Files\\Microsoft",
    ];
    for base in candidates {
        let p = Path::new(base);
        if p.exists() {
            // search shallow
            if let Ok(entries) = fs::read_dir(p) {
                for e in entries.flatten() {
                    let bin = e.path().join("bin").join("java.exe");
                    if bin.exists() { found.push(bin); }
                    // one more level
                    if let Ok(sub) = fs::read_dir(e.path()) {
                        for s in sub.flatten() {
                            let b2 = s.path().join("bin").join("java.exe");
                            if b2.exists() { found.push(b2); }
                        }
                    }
                }
            }
        }
    }
    // where java
    if let Ok(out) = new_cmd("where").arg("java").output() {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            let p = PathBuf::from(line.trim());
            if p.exists() { found.push(p); }
        }
    }

    // dedup
    let mut unique: Vec<PathBuf> = Vec::new();
    for p in found {
        if !unique.iter().any(|x| x == &p) {
            unique.push(p);
        }
    }
    let mut infos = Vec::new();
    for p in unique {
        infos.push(JavaInfo {
            path: p.to_string_lossy().to_string(),
            version: java_version(&p),
        });
    }
    Ok(infos)
}

#[tauri::command]
fn find_launch_script() -> Option<PathBuf> {
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../obsidia/scripts/launch_vanilla.py");
    if dev.exists() {
        return Some(dev);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for p in [dir.join("resources").join("launch_vanilla.py"), dir.join("launch_vanilla.py")] {
                if p.exists() {
                    return Some(p);
                }
            }
        }
    }
    None
}

#[tauri::command]
fn launch_version(
    version: String,
    username: String,
    ram: Option<String>,
    java_path: Option<String>,
) -> Result<String, String> {
    let script = find_launch_script().ok_or_else(|| "launch_vanilla.py not found".to_string())?;
    let script = script.canonicalize().map_err(|e| format!("script not found: {e}"))?;
    let mut cmd = new_cmd("python");
    cmd.arg(&script).arg(&version).arg("--username").arg(&username);
    if let Some(r) = ram {
        cmd.arg("--ram").arg(r);
    }
    if let Some(j) = java_path {
        cmd.arg("--java").arg(j);
    }
    let child = cmd.spawn().map_err(|e| format!("failed to spawn python: {e}"))?;
    Ok(format!("Lanzado {} para {} (pid {})", version, username, child.id()))
}

fn download_to(url: &str, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if dest.exists() {
        return Ok(());
    }
    let resp = reqwest::blocking::get(url).map_err(|e| e.to_string())?;
    let bytes = resp.bytes().map_err(|e| e.to_string())?;
    fs::write(dest, bytes).map_err(|e| e.to_string())?;
    Ok(())
}

fn java_exe() -> String {
    let javas = detect_javas().unwrap_or_default();
    if let Some(j) = javas.first() {
        j.path.clone()
    } else {
        "java".to_string()
    }
}

#[tauri::command]
fn install_fabric(mc_version: String) -> Result<String, String> {
    let installer_url = "https://maven.fabricmc.net/net/fabricmc/fabric-installer/1.1.2/fabric-installer-1.1.2.jar";
    let installer_path = minecraft_dir().join("obsidia").join("fabric-installer.jar");
    download_to(installer_url, &installer_path)?;
    let java = java_exe();
    let out = new_cmd(java)
        .arg("-jar")
        .arg(&installer_path)
        .arg("client")
        .arg("-mcversion")
        .arg(&mc_version)
        .arg("-dir")
        .arg(minecraft_dir())
        .output()
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(format!("fabric install failed: {stderr}\n{stdout}"));
    }
    Ok(format!("Fabric instalado para {mc_version}\n{stdout}"))
}

#[tauri::command]
fn install_forge(mc_version: String) -> Result<String, String> {
    // resolve forge version from promotions
    let promos: serde_json::Value = reqwest::blocking::get(
        "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json",
    )
    .map_err(|e| e.to_string())?
    .json()
    .map_err(|e| e.to_string())?;
    let key = format!("{mc_version}-latest");
    let forge_ver = promos
        .get("promos")
        .and_then(|p| p.get(&key))
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("no forge version for {mc_version}"))?
        .to_string();
    let url = format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{mc_version}-{forge_ver}/forge-{mc_version}-{forge_ver}-installer.jar"
    );
    let installer_path = minecraft_dir()
        .join("obsidia")
        .join(format!("forge-installer-{mc_version}.jar"));
    download_to(&url, &installer_path)?;
    let java = java_exe();
    let out = new_cmd(java)
        .arg("-jar")
        .arg(&installer_path)
        .arg("--installClient")
        .arg(minecraft_dir())
        .output()
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        return Err(format!("forge install failed: {stderr}\n{stdout}"));
    }
    Ok(format!("Forge {forge_ver} instalado para {mc_version}\n{stdout}"))
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ModResult {
    pub slug: String,
    pub title: String,
    pub description: String,
    pub downloads: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ServerEntry {
    pub name: String,
    pub address: String,
    #[serde(default)]
    pub version: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub ram: String,
    pub java_path: Option<String>,
    pub light_mode: bool,
    pub console: bool,
    pub username: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ram: "2G".to_string(),
            java_path: None,
            light_mode: false,
            console: false,
            username: "Player".to_string(),
        }
    }
}

fn settings_path() -> PathBuf {
    minecraft_dir().join("obsidia").join("settings.json")
}

fn servers_path() -> PathBuf {
    minecraft_dir().join("obsidia").join("servers.json")
}

#[tauri::command]
fn list_shaderpacks() -> Result<Vec<String>, String> {
    let dir = minecraft_dir().join("shaderpacks");
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.path().is_file() || e.path().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

#[tauri::command]
fn list_resourcepacks() -> Result<Vec<String>, String> {
    let dir = minecraft_dir().join("resourcepacks");
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.path().is_file() || e.path().is_dir() {
                if let Some(name) = e.file_name().to_str() {
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

#[tauri::command]
fn delete_file_in(folder: String, name: String) -> Result<(), String> {
    let path = minecraft_dir().join(folder).join(&name);
    if path.is_dir() {
        fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
    } else {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn list_installed_mods() -> Result<Vec<String>, String> {
    let dir = minecraft_dir().join("mods");
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.path().is_file() {
                if let Some(name) = e.file_name().to_str() {
                    out.push(name.to_string());
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

#[tauri::command]
async fn search_mods(query: String) -> Result<Vec<ModResult>, String> {
    let url = format!(
        "https://api.modrinth.com/v2/search?query={}&facets=[[\"project_type:mod\"]]&limit=20",
        urlencoding(&query)
    );
    let resp = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    if let Some(hits) = json.get("hits").and_then(|h| h.as_array()) {
        for h in hits {
            out.push(ModResult {
                slug: h.get("slug").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                title: h.get("title").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                description: h.get("description").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                downloads: h.get("downloads").and_then(|d| d.as_u64()).unwrap_or(0),
            });
        }
    }
    Ok(out)
}

fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' || c == '~' {
                c.to_string()
            } else if c == ' ' {
                "+".to_string()
            } else {
                format!("%{:02X}", c as u32)
            }
        })
        .collect()
}

#[tauri::command]
async fn install_mod(slug: String, mc_version: String, loader: String) -> Result<String, String> {
    let url = format!(
        "https://api.modrinth.com/v2/project/{}/version?game_versions=[\"{}\"]&loaders=[\"{}\"]",
        slug, mc_version, loader
    );
    let resp = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let versions = json.as_array().ok_or("no versions")?;
    let v = versions.first().ok_or("no compatible version")?;
    let files = v.get("files").and_then(|f| f.as_array()).ok_or("no files")?;
    let file = files
        .iter()
        .find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false))
        .or_else(|| files.first())
        .ok_or("no file")?;
    let file_url = file.get("url").and_then(|u| u.as_str()).ok_or("no url")?;
    let filename = file.get("filename").and_then(|u| u.as_str()).ok_or("no filename")?;
    let dest = minecraft_dir().join("mods").join(filename);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = reqwest::get(file_url).await.map_err(|e| e.to_string())?.bytes().await.map_err(|e| e.to_string())?;
    fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    Ok(format!("Mod instalado: {filename}"))
}

#[tauri::command]
fn get_servers() -> Result<Vec<ServerEntry>, String> {
    let path = servers_path();
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let servers: Vec<ServerEntry> = serde_json::from_str(&data).unwrap_or_default();
    Ok(servers)
}

#[tauri::command]
fn save_server(name: String, address: String, version: Option<String>) -> Result<(), String> {
    let path = servers_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut servers = get_servers().unwrap_or_default();
    servers.push(ServerEntry { name, address, version });
    let data = serde_json::to_string_pretty(&servers).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn delete_server(address: String) -> Result<(), String> {
    let path = servers_path();
    let mut servers = get_servers().unwrap_or_default();
    servers.retain(|s| s.address != address);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(&servers).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn ping_server(address: String) -> Result<String, String> {
    use std::net::TcpStream;
    use std::time::{Duration, Instant};
    let start = Instant::now();
    match TcpStream::connect_timeout(
        &address.parse().map_err(|e: std::net::AddrParseError| e.to_string())?,
        Duration::from_secs(5),
    ) {
        Ok(_) => Ok(format!("online ({} ms)", start.elapsed().as_millis())),
        Err(e) => Err(format!("offline: {e}")),
    }
}

#[tauri::command]
fn get_settings() -> Result<AppSettings, String> {
    let path = settings_path();
    if !path.exists() {
        return Ok(AppSettings::default());
    }
    let data = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let settings: AppSettings = serde_json::from_str(&data).unwrap_or_default();
    Ok(settings)
}

#[tauri::command]
fn save_settings(settings: AppSettings) -> Result<(), String> {
    let path = settings_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())?;
    Ok(())
}

const MS_CLIENT_ID: &str = "14d82eec-204b-4c2f-b7e8-296a70dab67e";

#[derive(Serialize, Deserialize, Clone)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct MsProfile {
    pub name: String,
    pub uuid: String,
    pub access_token: String,
}

fn auth_path() -> PathBuf {
    minecraft_dir().join("obsidia").join("auth.json")
}

#[tauri::command]
async fn ms_device_code() -> Result<DeviceCode, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
        .form(&[("client_id", MS_CLIENT_ID), ("scope", "XboxLive.signin offline_access")])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(DeviceCode {
        device_code: json.get("device_code").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        user_code: json.get("user_code").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        verification_uri: json.get("verification_uri").and_then(|v| v.as_str()).unwrap_or("").to_string(),
        expires_in: json.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(900),
        interval: json.get("interval").and_then(|v| v.as_u64()).unwrap_or(5),
    })
}

#[tauri::command]
async fn ms_login(device_code: String) -> Result<MsProfile, String> {
    let client = reqwest::Client::new();
    // poll token
    let mut interval = 5u64;
    let mut ms_access = String::new();
    for _ in 0..120 {
        std::thread::sleep(Duration::from_secs(interval));
        let resp = client
            .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", MS_CLIENT_ID),
                ("device_code", &device_code),
            ])
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        if let Some(tok) = json.get("access_token").and_then(|v| v.as_str()) {
            ms_access = tok.to_string();
            break;
        }
        let err = json.get("error").and_then(|v| v.as_str()).unwrap_or("");
        if err == "authorization_pending" {
            continue;
        }
        if err == "slow_down" {
            interval += 5;
            continue;
        }
        return Err(format!("token error: {err} {}", json.get("error_description").and_then(|v| v.as_str()).unwrap_or("")));
    }
    if ms_access.is_empty() {
        return Err("timeout waiting for login".to_string());
    }

    // xbox live
    let xbl_body = serde_json::json!({
        "Properties": {
            "AuthMethod": "RPS",
            "SiteName": "user.auth.xboxlive.com",
            "RpsTicket": format!("d={}", ms_access)
        },
        "RelyingParty": "http://auth.xboxlive.com",
        "TokenType": "JWT"
    });
    let xbl_resp: serde_json::Value = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&xbl_body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let xbl_token = xbl_resp.get("Token").and_then(|v| v.as_str()).ok_or("no xbl token")?.to_string();
    let uhs = xbl_resp
        .get("DisplayClaims")
        .and_then(|d| d.get("xui"))
        .and_then(|x| x.as_array())
        .and_then(|a| a.first())
        .and_then(|o| o.get("uhs"))
        .and_then(|v| v.as_str())
        .ok_or("no uhs")?
        .to_string();

    // xsts
    let xsts_body = serde_json::json!({
        "Properties": {
            "SandboxId": "RETAIL",
            "UserTokens": [xbl_token]
        },
        "RelyingParty": "rp://api.minecraftservices.com/",
        "TokenType": "JWT"
    });
    let xsts_resp: serde_json::Value = client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&xsts_body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let xsts_token = xsts_resp.get("Token").and_then(|v| v.as_str()).ok_or("no xsts token")?.to_string();

    // minecraft login
    let mc_body = serde_json::json!({
        "identityToken": format!("XBL3.0 x={};{}", uhs, xsts_token)
    });
    let mc_resp: serde_json::Value = client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .header("Content-Type", "application/json")
        .json(&mc_body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let mc_access = mc_resp.get("access_token").and_then(|v| v.as_str()).ok_or("no mc access")?.to_string();

    // profile
    let profile_resp: serde_json::Value = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(&mc_access)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let name = profile_resp.get("name").and_then(|v| v.as_str()).unwrap_or("Player").to_string();
    let uuid = profile_resp.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let profile = MsProfile { name, uuid, access_token: mc_access };
    // save
    let path = auth_path();
    if let Some(parent) = path.parent() { let _ = fs::create_dir_all(parent); }
    if let Ok(data) = serde_json::to_string_pretty(&profile) { let _ = fs::write(&path, data); }
    Ok(profile)
}

#[tauri::command]
fn ms_get_profile() -> Result<Option<MsProfile>, String> {
    let path = auth_path();
    if !path.exists() { return Ok(None); }
    let data = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let profile: MsProfile = serde_json::from_str(&data).map_err(|e| e.to_string())?;
    Ok(Some(profile))
}

#[tauri::command]
fn ms_logout() -> Result<(), String> {
    let path = auth_path();
    if path.exists() { let _ = fs::remove_file(&path); }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_installed_versions,
            list_versions,
            get_profiles,
            save_profile,
            delete_profile,
            detect_javas,
            launch_version,
            install_fabric,
            install_forge,
            list_shaderpacks,
            list_resourcepacks,
            delete_file_in,
            list_installed_mods,
            search_mods,
            install_mod,
            get_servers,
            save_server,
            delete_server,
            ping_server,
            get_settings,
            save_settings,
            ms_device_code,
            ms_login,
            ms_get_profile,
            ms_logout
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}