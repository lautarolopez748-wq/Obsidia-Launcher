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
    #[serde(default)]
    pub fullscreen: bool,
    /// Identificador de la instancia (carpeta propia con mods, packs, config y mundos).
    /// Los perfiles viejos no lo traen y siguen usando `.minecraft` tal cual.
    #[serde(default)]
    pub id: Option<String>,
}

/// Carpeta de juego de una instancia. Sin id (perfiles viejos) es `.minecraft`; con id es
/// `.minecraft/obsidia/instances/<id>`. `libraries`, `assets`, `versions` y `runtime` siguen
/// compartidos en `.minecraft` para no bajar lo mismo varias veces.
fn game_dir(instance: Option<&str>) -> Result<PathBuf, String> {
    match instance.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(minecraft_dir()),
        Some(id) => {
            if id.len() > 48 || !only_chars(id, &['-', '_']) {
                return Err("Identificador de instancia inválido".to_string());
            }
            Ok(minecraft_dir().join("obsidia").join("instances").join(id))
        }
    }
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

/// Lee una lista JSON. Si el archivo no existe devuelve una lista vacía; si existe pero está
/// dañado NO lo da por vacío (el siguiente guardado lo borraría): hace una copia `.bak` y
/// devuelve un error para que nadie lo pise.
fn read_json_list<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>, String> {
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&data).map_err(|e| {
        let backup = path.with_extension("json.bak");
        let _ = fs::copy(path, &backup);
        format!(
            "{} está dañado ({e}). Guardé una copia en {} y no lo toqué.",
            path.display(),
            backup.display()
        )
    })
}

/// Escribe un archivo de forma atómica: primero a `.tmp` y después se renombra encima.
fn write_atomic(path: &Path, data: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, data).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

fn write_json_list<T: Serialize>(path: &Path, list: &[T]) -> Result<(), String> {
    let data = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    write_atomic(path, &data)
}

#[tauri::command]
fn get_profiles() -> Result<Vec<Profile>, String> {
    read_json_list(&profiles_path())
}

#[tauri::command]
fn save_profile(profile: Profile) -> Result<(), String> {
    let path = profiles_path();
    let mut profiles = read_json_list::<Profile>(&path)?;
    if let Some(existing) = profiles.iter_mut().find(|p| p.name == profile.name) {
        *existing = profile;
    } else {
        profiles.push(profile);
    }
    write_json_list(&path, &profiles)
}

#[tauri::command]
fn delete_profile(name: String) -> Result<(), String> {
    let path = profiles_path();
    let mut profiles = read_json_list::<Profile>(&path)?;
    profiles.retain(|p| p.name != name);
    write_json_list(&path, &profiles)
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
    fullscreen: Option<bool>,
    instance: Option<String>,
) -> Result<String, String> {
    let gd = game_dir(instance.as_deref())?;
    let player = match auth::player().await? {
        Some(p) => p,
        None => launcher::offline_player(&username)?,
    };
    let mc = minecraft_dir();
    let result = tauri::async_runtime::spawn_blocking(move || {
        launcher::launch(&app, &mc, &gd, &version, player, ram, java_path, java_exe, server, fullscreen.unwrap_or(false))
    })
    .await
    .map_err(|e| e.to_string())?;
    result
}

fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// Descarga un instalador y comprueba su SHA1 contra el archivo `<url>.sha1` que publica el
/// mismo Maven. Si ya está descargado y coincide no se vuelve a bajar; nunca se ejecuta un
/// archivo cuyo hash no se pudo comprobar.
fn download_verified(url: &str, dest: &Path) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent(launcher::UA)
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;

    let sum = client
        .get(format!("{url}.sha1"))
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.text())
        .map_err(|e| format!("No pude obtener el SHA1 de {url}: {e}"))?;
    let expected = sum.split_whitespace().next().unwrap_or("").to_lowercase();
    if expected.len() != 40 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("El SHA1 publicado para {url} no es válido"));
    }

    if let Ok(existing) = fs::read(dest) {
        if sha1_hex(&existing) == expected {
            return Ok(());
        }
    }

    let bytes = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.bytes())
        .map_err(|e| format!("{url}: {e}"))?;
    if sha1_hex(&bytes) != expected {
        return Err(format!("El instalador descargado de {url} no coincide con su SHA1: se descartó"));
    }

    let mut tmp = dest.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    if dest.exists() {
        let _ = fs::remove_file(dest);
    }
    fs::rename(&tmp, dest).map_err(|e| format!("{}: {e}", dest.display()))
}

/// Respaldo si no se puede consultar la lista de instaladores de Fabric.
const FABRIC_INSTALLER_FALLBACK: (&str, &str) = (
    "1.1.2",
    "https://maven.fabricmc.net/net/fabricmc/fabric-installer/1.1.2/fabric-installer-1.1.2.jar",
);

/// (versión, url) del instalador estable más nuevo de Fabric.
fn fabric_installer() -> (String, String) {
    let fallback = || (FABRIC_INSTALLER_FALLBACK.0.to_string(), FABRIC_INSTALLER_FALLBACK.1.to_string());
    let Ok(client) = reqwest::blocking::Client::builder()
        .user_agent(launcher::UA)
        .timeout(Duration::from_secs(30))
        .build()
    else {
        return fallback();
    };
    let list: Option<serde_json::Value> = client
        .get("https://meta.fabricmc.net/v2/versions/installer")
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .ok();
    list.as_ref()
        .and_then(|l| l.as_array())
        .and_then(|a| a.iter().find(|v| v.get("stable").and_then(|s| s.as_bool()) == Some(true)))
        .and_then(|v| {
            let ver = v.get("version")?.as_str()?;
            let url = v.get("url")?.as_str()?;
            // Solo desde el Maven oficial de Fabric y con un nombre de versión simple.
            let ok = url.starts_with("https://maven.fabricmc.net/")
                && !ver.is_empty()
                && ver.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
            ok.then(|| (ver.to_string(), url.to_string()))
        })
        .unwrap_or_else(fallback)
}

/// Versión de Minecraft que va a una URL y al nombre de un archivo: se valida antes de usarla.
fn check_mc_version(v: &str) -> Result<(), String> {
    if only_chars(v, &['.', '-', '_']) {
        Ok(())
    } else {
        Err("Versión de Minecraft inválida".to_string())
    }
}

fn java_exe() -> String {
    let javas = detect_javas().unwrap_or_default();
    if let Some(j) = javas.first() {
        j.path.clone()
    } else {
        "java".to_string()
    }
}

fn ensure_launcher_profiles() {
    let path = minecraft_dir().join("launcher_profiles.json");
    if !path.exists() {
        let _ = std::fs::write(&path, "{\"profiles\":{},\"settings\":{}}");
    }
}

#[tauri::command(async)]
fn install_fabric(mc_version: String) -> Result<String, String> {
    check_mc_version(&mc_version)?;
    ensure_launcher_profiles();
    let (installer_ver, installer_url) = fabric_installer();
    let installer_path = minecraft_dir()
        .join("obsidia")
        .join(format!("fabric-installer-{installer_ver}.jar"));
    download_verified(&installer_url, &installer_path)?;
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
    check_mc_version(&mc_version)?;
    ensure_launcher_profiles();
    // resolve forge version from promotions
    let resp = reqwest::blocking::get(
        "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json",
    )
    .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("forge promotions request failed: HTTP {}", resp.status()));
    }
    let promos: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
    // Se prefiere la versión recomendada y, si no hay, la más nueva.
    let promo = |kind: &str| {
        promos
            .get("promos")
            .and_then(|p| p.get(format!("{mc_version}-{kind}")))
            .and_then(|v| v.as_str())
            .map(String::from)
    };
    let forge_ver = promo("recommended")
        .or_else(|| promo("latest"))
        .ok_or_else(|| format!("no forge version for {mc_version}"))?;
    if !only_chars(&forge_ver, &['.', '-', '_']) {
        return Err("Versión de Forge inválida".to_string());
    }
    let url = format!(
        "https://maven.minecraftforge.net/net/minecraftforge/forge/{mc_version}-{forge_ver}/forge-{mc_version}-{forge_ver}-installer.jar"
    );
    let installer_path = minecraft_dir()
        .join("obsidia")
        .join(format!("forge-installer-{mc_version}.jar"));
    download_verified(&url, &installer_path)?;
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
fn list_shaderpacks(instance: Option<String>) -> Result<Vec<String>, String> {
    let dir = game_dir(instance.as_deref())?.join("shaderpacks");
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
fn list_resourcepacks(instance: Option<String>) -> Result<Vec<String>, String> {
    let dir = game_dir(instance.as_deref())?.join("resourcepacks");
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
fn get_resourcepack_state(instance: Option<String>) -> Result<Vec<packs::PackState>, String> {
    Ok(packs::get_state(&game_dir(instance.as_deref())?))
}

/// Guarda qué packs están activos y en qué orden (la lista llega ya priorizada).
#[tauri::command(async)]
fn save_resourcepack_state(active: Vec<String>, instance: Option<String>) -> Result<(), String> {
    packs::save_state(&game_dir(instance.as_deref())?, active)
}

/// Genera el resource pack `ObsidiaSkin` y lo activa en `options.txt`.
/// Solo tiene sentido en modo offline: con cuenta Microsoft el juego usa la skin oficial.
#[tauri::command(async)]
fn apply_offline_skin(
    skin_png_base64: String,
    model: String,
    mc_version: Option<String>,
    instance: Option<String>,
) -> Result<String, String> {
    if auth::has_session() {
        return Err(
            "Tenés una cuenta Microsoft: el juego ya usa tu skin oficial y no se aplica el pack offline."
                .to_string(),
        );
    }
    let png = skin::decode_skin(&skin_png_base64)?;
    skin::normalize_model(&model)?;
    skin::apply_skin(&game_dir(instance.as_deref())?, &png, &model, mc_version.as_deref())?;
    Ok("Skin aplicada al juego (resource pack ObsidiaSkin).".to_string())
}

/// Borra el resource pack de skin y su entrada de `options.txt`.
#[tauri::command(async)]
fn remove_offline_skin(instance: Option<String>) -> Result<(), String> {
    skin::remove_skin(&game_dir(instance.as_deref())?)
}

#[tauri::command]
fn delete_file_in(folder: String, name: String, instance: Option<String>) -> Result<(), String> {
    // Solo estas carpetas, y solo un nombre simple (sin "..", barras ni letras de unidad).
    if !["mods", "shaderpacks", "resourcepacks"].contains(&folder.as_str()) {
        return Err("Carpeta no permitida".to_string());
    }
    let mut comps = Path::new(&name).components();
    match (comps.next(), comps.next()) {
        (Some(std::path::Component::Normal(_)), None) => {}
        _ => return Err("Nombre de archivo inválido".to_string()),
    }
    let path = game_dir(instance.as_deref())?.join(folder).join(&name);
    if path.is_dir() {
        fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
    } else {
        fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn list_installed_mods(instance: Option<String>) -> Result<Vec<String>, String> {
    let dir = game_dir(instance.as_deref())?.join("mods");
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

/// Busca proyectos en Modrinth (`project_type`: mod, shader o resourcepack).
async fn search_modrinth(query: String, project_type: &str) -> Result<Vec<ModResult>, String> {
    let facets = format!("[[\"project_type:{project_type}\"]]");
    let resp = http()?
        .get("https://api.modrinth.com/v2/search")
        .query(&[("query", query.as_str()), ("facets", facets.as_str()), ("limit", "20")])
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let text = |h: &serde_json::Value, k: &str| h.get(k).and_then(|s| s.as_str()).unwrap_or("").to_string();
    let mut out = Vec::new();
    if let Some(hits) = json.get("hits").and_then(|h| h.as_array()) {
        for h in hits {
            out.push(ModResult {
                slug: text(h, "slug"),
                title: text(h, "title"),
                description: text(h, "description"),
                downloads: h.get("downloads").and_then(|d| d.as_u64()).unwrap_or(0),
            });
        }
    }
    Ok(out)
}

#[tauri::command]
async fn search_mods(query: String) -> Result<Vec<ModResult>, String> {
    search_modrinth(query, "mod").await
}

#[tauri::command]
async fn search_shaders(query: String) -> Result<Vec<ModResult>, String> {
    search_modrinth(query, "shader").await
}

#[tauri::command]
async fn search_resourcepacks(query: String) -> Result<Vec<ModResult>, String> {
    search_modrinth(query, "resourcepack").await
}

/// Modrinth devuelve primero lo más nuevo, alphas y betas incluidas: se prefiere la
/// primera versión estable y solo si no hay ninguna se usa la primera de la lista.
fn pick_version(versions: &[serde_json::Value]) -> Option<&serde_json::Value> {
    versions
        .iter()
        .find(|v| v.get("version_type").and_then(|t| t.as_str()) == Some("release"))
        .or_else(|| versions.first())
}

fn only_chars(s: &str, extra: &[char]) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(&c))
}

#[tauri::command]
async fn install_mod(
    slug: String,
    mc_version: String,
    loader: String,
    instance: Option<String>,
) -> Result<String, String> {
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
    let v = pick_version(versions).ok_or("No hay una versión compatible de este mod")?;
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

    let dest = game_dir(instance.as_deref())?.join("mods").join(filename);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(&dest, bytes).map_err(|e| e.to_string())?;
    Ok(format!("Mod instalado: {filename}"))
}

#[tauri::command]
async fn install_shaderpack(
    slug: String,
    mc_version: Option<String>,
    instance: Option<String>,
) -> Result<String, String> {
    download_project_file(&slug, "shaderpacks", mc_version.as_deref(), instance.as_deref()).await
}

#[tauri::command(async)]
async fn install_resourcepack(
    slug: String,
    mc_version: Option<String>,
    instance: Option<String>,
) -> Result<String, String> {
    download_project_file(&slug, "resourcepacks", mc_version.as_deref(), instance.as_deref()).await
}

async fn download_project_file(
    slug: &str,
    folder: &str,
    mc_version: Option<&str>,
    instance: Option<&str>,
) -> Result<String, String> {
    if !only_chars(slug, &['-', '_']) {
        return Err("Slug inválido".to_string());
    }
    if !["shaderpacks", "resourcepacks"].contains(&folder) {
        return Err("Carpeta inválida".to_string());
    }
    let client = http()?;
    let mut req = client.get(format!("https://api.modrinth.com/v2/project/{slug}/version"));
    if let Some(v) = mc_version {
        req = req.query(&[("game_versions", format!("[\"{v}\"]"))]);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let versions = json.as_array().ok_or("Modrinth no devolvió versiones")?;
    let v = pick_version(versions).ok_or("No hay una versión compatible")?;
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
        (Some(std::path::Component::Normal(_)), None) if filename.to_lowercase().ends_with(".zip") => {}
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

    if let Some(expected) = file
        .get("hashes")
        .and_then(|h| h.get("sha1"))
        .and_then(|h| h.as_str())
    {
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(&bytes);
        if !format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(expected) {
            return Err("El archivo descargado no coincide con su SHA1: se descartó".to_string());
        }
    }

    let dest = game_dir(instance)?.join(folder).join(filename);
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
    read_json_list(&servers_path())
}

#[tauri::command]
fn save_server(name: String, address: String, version: Option<String>) -> Result<(), String> {
    let path = servers_path();
    let mut servers = read_json_list::<ServerEntry>(&path)?;
    // Misma dirección = se actualiza la entrada en vez de duplicarla.
    if let Some(existing) = servers.iter_mut().find(|s| s.address == address) {
        existing.name = name;
        existing.version = version;
    } else {
        servers.push(ServerEntry { name, address, version });
    }
    write_json_list(&path, &servers)
}

#[tauri::command]
fn delete_server(address: String) -> Result<(), String> {
    let path = servers_path();
    let mut servers = read_json_list::<ServerEntry>(&path)?;
    servers.retain(|s| s.address != address);
    write_json_list(&path, &servers)
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
    write_atomic(&path, &data)
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
fn ms_cancel() {
    auth::cancel_login();
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
            ms_cancel,
            ms_get_profile,
            ms_logout
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("obsidia-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn json_danado_no_se_pisa_y_queda_respaldo() {
        let dir = tmp_dir("danado");
        let path = dir.join("profiles.json");
        fs::write(&path, "{ esto no es json").unwrap();
        assert!(read_json_list::<Profile>(&path).is_err());
        // El original sigue intacto y hay una copia de respaldo.
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ esto no es json");
        assert!(dir.join("profiles.json.bak").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archivo_inexistente_es_lista_vacia() {
        let dir = tmp_dir("vacio");
        assert!(read_json_list::<Profile>(&dir.join("nada.json")).unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn escritura_atomica_ida_y_vuelta() {
        let dir = tmp_dir("atomica");
        let path = dir.join("sub").join("servers.json");
        let lista = vec![ServerEntry { name: "A".into(), address: "a.net".into(), version: None }];
        write_json_list(&path, &lista).unwrap();
        let leida: Vec<ServerEntry> = read_json_list(&path).unwrap();
        assert_eq!(leida.len(), 1);
        assert_eq!(leida[0].address, "a.net");
        assert!(!dir.join("sub").join("servers.json.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn carpeta_de_instancia() {
        // Sin id: perfiles viejos, siguen en .minecraft.
        assert_eq!(game_dir(None).unwrap(), minecraft_dir());
        assert_eq!(game_dir(Some("  ")).unwrap(), minecraft_dir());
        // Con id: carpeta propia dentro de obsidia/instances.
        let d = game_dir(Some("pabc-123_x")).unwrap();
        assert!(d.ends_with(Path::new("obsidia").join("instances").join("pabc-123_x")));
        // Ids que intentan salirse de la carpeta se rechazan.
        assert!(game_dir(Some("../etc")).is_err());
        assert!(game_dir(Some("a/b")).is_err());
        assert!(game_dir(Some("a\\b")).is_err());
        assert!(game_dir(Some("C:")).is_err());
    }

    #[test]
    fn version_de_minecraft_valida() {
        assert!(check_mc_version("1.21.4").is_ok());
        assert!(check_mc_version("24w14a").is_ok());
        assert!(check_mc_version("../x").is_err());
        assert!(check_mc_version("1.21/4").is_err());
        assert!(check_mc_version("").is_err());
    }

    #[test]
    fn sha1_conocido() {
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    #[test]
    fn prefiere_versiones_release() {
        let v = vec![
            json!({"id":"beta","version_type":"beta"}),
            json!({"id":"rel","version_type":"release"}),
        ];
        assert_eq!(pick_version(&v).unwrap()["id"], "rel");
        let solo_beta = vec![json!({"id":"beta","version_type":"beta"})];
        assert_eq!(pick_version(&solo_beta).unwrap()["id"], "beta");
        assert!(pick_version(&[]).is_none());
    }
}
