mod auth;
mod launcher;
mod packs;
mod skin;

use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

pub(crate) fn new_cmd<S: AsRef<std::ffi::OsStr>>(program: S) -> Command {
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
    /// "Vanilla", "Fabric" o "Forge". Los perfiles viejos no lo traen: queda en None.
    #[serde(default)]
    pub loader: Option<String>,
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

#[tauri::command(async)]
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

/// Abre el juego (descargando lo que falte). Si hay sesión de Microsoft iniciada se usa esa
/// cuenta; si no, se juega offline con `username`. El progreso llega por el evento
/// `launch-progress` ({ stage, done, total }).
#[tauri::command]
async fn launch_version(
    app: tauri::AppHandle,
    version: String,
    username: String,
    ram: Option<String>,
    java_path: Option<String>,
    server: Option<String>,
) -> Result<String, String> {
    let player = match auth::player().await? {
        Some(p) => p,
        None => launcher::offline_player(&username)?,
    };
    let mc = minecraft_dir();
    let fallback = java_exe();
    let result = tauri::async_runtime::spawn_blocking(move || {
        launcher::launch(&app, &mc, &version, player, ram, java_path, fallback, server)
    })
    .await
    .map_err(|e| e.to_string())?;
    result
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

#[tauri::command(async)]
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

#[tauri::command(async)]
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

/// Estado de los resource packs: activos en orden de prioridad y el resto.
#[tauri::command(async)]
fn get_resourcepack_state() -> Result<Vec<packs::PackState>, String> {
    Ok(packs::get_state(&minecraft_dir()))
}

/// Guarda qué packs están activos y en qué orden (la lista llega ya priorizada).
#[tauri::command(async)]
fn save_resourcepack_state(active: Vec<String>) -> Result<(), String> {
    packs::save_state(&minecraft_dir(), active)
}

/// Genera el resource pack `ObsidiaSkin` y lo activa en `options.txt`.
/// Solo tiene sentido en modo offline: con cuenta Microsoft el juego usa la skin oficial.
#[tauri::command(async)]
fn apply_offline_skin(
    skin_png_base64: String,
    model: String,
    mc_version: Option<String>,
) -> Result<String, String> {
    if auth::has_session() {
        return Err(
            "Tenés una cuenta Microsoft: el juego ya usa tu skin oficial y no se aplica el pack offline."
                .to_string(),
        );
    }
    let png = skin::decode_skin(&skin_png_base64)?;
    skin::normalize_model(&model)?;
    skin::apply_skin(&minecraft_dir(), &png, &model, mc_version.as_deref())?;
    Ok("Skin aplicada al juego (resource pack ObsidiaSkin).".to_string())
}

/// Borra el resource pack de skin y su entrada de `options.txt`.
#[tauri::command(async)]
fn remove_offline_skin() -> Result<(), String> {
    skin::remove_skin(&minecraft_dir())
}

#[tauri::command]
fn delete_file_in(folder: String, name: String) -> Result<(), String> {
    // Solo estas carpetas, y solo un nombre simple (sin "..", barras ni letras de unidad).
    if !["mods", "shaderpacks", "resourcepacks"].contains(&folder.as_str()) {
        return Err("Carpeta no permitida".to_string());
    }
    let mut comps = Path::new(&name).components();
    match (comps.next(), comps.next()) {
        (Some(std::path::Component::Normal(_)), None) => {}
        _ => return Err("Nombre de archivo inválido".to_string()),
    }
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

/// Cliente HTTP con User-Agent propio (Modrinth lo exige).
fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(launcher::UA)
        .build()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn search_mods(query: String) -> Result<Vec<ModResult>, String> {
    let resp = http()?
        .get("https://api.modrinth.com/v2/search")
        .query(&[
            ("query", query.as_str()),
            ("facets", "[[\"project_type:mod\"]]"),
            ("limit", "20"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
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

#[tauri::command]
async fn search_shaders(query: String) -> Result<Vec<ModResult>, String> {
    let resp = http()?
        .get("https://api.modrinth.com/v2/search")
        .query(&[
            ("query", query.as_str()),
            ("facets", "[[\"project_type:shader\"]]"),
            ("limit", "20"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
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

#[tauri::command]
async fn search_resourcepacks(query: String) -> Result<Vec<ModResult>, String> {
    let resp = http()?
        .get("https://api.modrinth.com/v2/search")
        .query(&[
            ("query", query.as_str()),
            ("facets", "[[\"project_type:resourcepack\"]]"),
            ("limit", "20"),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
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

fn only_chars(s: &str, extra: &[char]) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(&c))
}

#[tauri::command]
async fn install_mod(slug: String, mc_version: String, loader: String) -> Result<String, String> {
    // Los tres valores van a una URL y a una ruta: se validan antes de usarlos.
    if !only_chars(&slug, &['-', '_']) || !only_chars(&mc_version, &['.', '-', '_']) {
        return Err("Datos del mod inválidos".to_string());
    }
    if !["fabric", "forge", "neoforge", "quilt"].contains(&loader.as_str()) {
        return Err("Loader no soportado".to_string());
    }
    let client = http()?;
    let resp = client
        .get(format!("https://api.modrinth.com/v2/project/{slug}/version"))
        .query(&[
            ("game_versions", format!("[\"{mc_version}\"]")),
            ("loaders", format!("[\"{loader}\"]")),
        ])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let versions = json.as_array().ok_or("Modrinth no devolvió versiones")?;
    let v = versions.first().ok_or("No hay una versión compatible de este mod")?;
    let files = v.get("files").and_then(|f| f.as_array()).ok_or("El mod no trae archivos")?;
    let file = files
        .iter()
        .find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false))
        .or_else(|| files.first())
        .ok_or("El mod no trae archivos")?;
    let file_url = file.get("url").and_then(|u| u.as_str()).ok_or("El archivo no tiene URL")?;
    let filename = file.get("filename").and_then(|u| u.as_str()).ok_or("El archivo no tiene nombre")?;

    // El nombre viene de un servidor externo: solo un nombre simple que termine en .jar.
    let mut comps = Path::new(filename).components();
    match (comps.next(), comps.next()) {
        (Some(std::path::Component::Normal(_)), None) if filename.to_lowercase().ends_with(".jar") => {}
        _ => return Err("Nombre de archivo sospechoso en la respuesta de Modrinth".to_string()),
    }

    let bytes = client
        .get(file_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    // Verificación de integridad con el SHA1 que informa Modrinth.
    if let Some(expected) = file
        .get("hashes")
        .and_then(|h| h.get("sha1"))
        .and_then(|h| h.as_str())
    {
        let mut hasher = Sha1::new();
        hasher.update(&bytes);
        if !format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(expected) {
            return Err("El archivo descargado no coincide con su SHA1: se descartó".to_string());
        }
    }

    let dest = minecraft_dir().join("mods").join(filename);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    Ok(format!("Mod instalado: {filename}"))
}

#[tauri::command]
async fn install_shaderpack(slug: String) -> Result<String, String> {
    download_project_file(&slug, "shaderpacks").await
}

#[tauri::command]
async fn install_resourcepack(slug: String) -> Result<String, String> {
    download_project_file(&slug, "resourcepacks").await
}

async fn download_project_file(slug: &str, folder: &str) -> Result<String, String> {
    if !only_chars(slug, &['-', '_']) {
        return Err("Slug inválido".to_string());
    }
    if !["shaderpacks", "resourcepacks"].contains(&folder) {
        return Err("Carpeta inválida".to_string());
    }
    let client = http()?;
    let resp = client
        .get(format!("https://api.modrinth.com/v2/project/{slug}/version"))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let versions = json.as_array().ok_or("Modrinth no devolvió versiones")?;
    let v = versions.first().ok_or("No hay una versión compatible")?;
    let files = v.get("files").and_then(|f| f.as_array()).ok_or("El proyecto no trae archivos")?;
    let file = files
        .iter()
        .find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false))
        .or_else(|| files.first())
        .ok_or("El proyecto no trae archivos")?;
    let file_url = file.get("url").and_then(|u| u.as_str()).ok_or("El archivo no tiene URL")?;
    let filename = file.get("filename").and_then(|u| u.as_str()).ok_or("El archivo no tiene nombre")?;

    let mut comps = Path::new(filename).components();
    match (comps.next(), comps.next()) {
        (Some(std::path::Component::Normal(_)), None) => {}
        _ => return Err("Nombre de archivo sospechoso".to_string()),
    }

    let bytes = client
        .get(file_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?
        .bytes()
        .await
        .map_err(|e| e.to_string())?;

    let dest = minecraft_dir().join(folder).join(filename);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    Ok(filename.to_string())
}

#[tauri::command(async)]
fn get_total_ram() -> Result<u64, String> {
    #[cfg(windows)]
    {
        if let Ok(out) = new_cmd("powershell")
            .args(["-NoProfile", "-Command", "(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory"])
            .output()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Ok(bytes) = text.trim().parse::<u64>() {
                let gb = bytes / (1024 * 1024 * 1024);
                return Ok(gb.max(2));
            }
        }
    }
    Ok(16)
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
    // Misma dirección = se actualiza la entrada en vez de duplicarla.
    if let Some(existing) = servers.iter_mut().find(|s| s.address == address) {
        existing.name = name;
        existing.version = version;
    } else {
        servers.push(ServerEntry { name, address, version });
    }
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

/// Acepta `host`, `host:puerto` o `ip:puerto` (puerto 25565 por defecto).
#[tauri::command(async)]
fn ping_server(address: String) -> Result<String, String> {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Instant;
    let address = address.trim();
    let with_port = if address.contains(':') {
        address.to_string()
    } else {
        format!("{address}:25565")
    };
    let sock = with_port
        .to_socket_addrs()
        .map_err(|e| format!("dirección inválida: {e}"))?
        .next()
        .ok_or("no se pudo resolver la dirección")?;
    let start = Instant::now();
    match TcpStream::connect_timeout(&sock, Duration::from_secs(5)) {
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

// El login de Microsoft vive en auth.rs: los tokens van al Credential Manager.

#[tauri::command]
async fn ms_device_code() -> Result<auth::DeviceCode, String> {
    auth::request_device_code().await
}

#[tauri::command]
async fn ms_login(device_code: String) -> Result<auth::MsProfile, String> {
    auth::wait_for_login(&device_code).await
}

#[tauri::command]
async fn ms_get_profile() -> Result<Option<auth::MsProfile>, String> {
    auth::current_profile().await
}

#[tauri::command]
fn ms_logout() -> Result<(), String> {
    auth::logout()
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
            get_resourcepack_state,
            save_resourcepack_state,
            apply_offline_skin,
            remove_offline_skin,
            delete_file_in,
            list_installed_mods,
            search_mods,
            search_shaders,
            search_resourcepacks,
            install_mod,
            install_shaderpack,
            install_resourcepack,
            get_total_ram,
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