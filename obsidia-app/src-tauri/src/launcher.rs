//! Lanzamiento de Minecraft Java en Rust (reemplaza a `launch_vanilla.py`).
//!
//! - Descarga lo que falte (versión, librerías, assets) en paralelo y verifica SHA1.
//! - Soporta vanilla y versiones con `inheritsFrom` (Fabric / Forge ya instalados).
//! - Extrae las natives, arma el classpath y abre el juego con la sesión indicada
//!   (offline o Microsoft). La salida del juego va a `obsidia/logs/latest.log`.

use serde::Serialize;
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

use crate::options;

/// Instancias en ejecución: id → pid.
pub struct RunningInstances(pub Mutex<HashMap<String, u32>>);

impl Default for RunningInstances {
    fn default() -> Self {
        Self(Mutex::new(HashMap::new()))
    }
}

impl RunningInstances {
    /// Intenta reservar una instancia. Devuelve `true` si la reserva se hizo,
    /// `false` si ya estaba en ejecución.
    pub fn reserve(&self, id: &str) -> bool {
        let mut guard = self.0.lock().unwrap();
        if guard.contains_key(id) {
            return false;
        }
        guard.insert(id.to_string(), 0);
        true
    }

    /// Registrar el PID real de una instancia ya reservada.
    pub fn set_pid(&self, id: &str, pid: u32) {
        if let Ok(mut guard) = self.0.lock()
            && let Some(entry) = guard.get_mut(id)
        {
            *entry = pid;
        }
    }

    /// Liberar la reserva de una instancia si el PID coincide.
    /// Devuelve `true` si liberó, `false` si el PID era de otro proceso.
    pub fn release_if_matches(&self, id: &str, pid: u32) -> bool {
        if let Ok(mut guard) = self.0.lock()
            && let Some(&existing) = guard.get(id)
            && (existing == pid || existing == 0)
        {
            guard.remove(id);
            return true;
        }
        false
    }

    /// Liberar la reserva incondicionalmente (para errores de lanzamiento).
    pub fn release(&self, id: &str) {
        if let Ok(mut guard) = self.0.lock() {
            guard.remove(id);
        }
    }

    /// Verificar si una instancia está en ejecución.
    #[allow(dead_code)]
    pub fn is_running(&self, id: &str) -> bool {
        self.0.lock().unwrap().contains_key(id)
    }
}

pub const UA: &str = "Obsidia/0.1 (+https://github.com/lautarolopez748-wq/Obsidia-Launcher)";
const MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const ASSET_BASE: &str = "https://resources.download.minecraft.net";
const LIB_BASE: &str = "https://libraries.minecraft.net";
const DOWNLOAD_THREADS: usize = 8;
/// Margen corto para detectar un Java que falla al arrancar antes de informar éxito.
const STARTUP_GRACE: Duration = Duration::from_secs(2);
const RUNTIME_ALL: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

const CP_SEP: &str = if cfg!(windows) { ";" } else { ":" };

// ───────────────────────────── Jugador / sesión ──────────────────────────────

#[derive(Clone)]
pub struct Player {
    pub name: String,
    pub uuid: String,
    pub token: String,
    pub user_type: String,
}

/// 3 a 16 caracteres: letras, números y guion bajo.
pub fn validate_player(name: &str) -> Result<(), String> {
    let len = name.chars().count();
    if (3..=16).contains(&len) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(())
    } else {
        Err("El nombre de jugador debe tener entre 3 y 16 caracteres (letras, números y _)".into())
    }
}

/// UUID offline como lo calcula Minecraft: UUID v3 de "OfflinePlayer:<nombre>".
pub fn offline_uuid(name: &str) -> String {
    let mut h = md5::compute(format!("OfflinePlayer:{name}").as_bytes()).0;
    h[6] = (h[6] & 0x0f) | 0x30;
    h[8] = (h[8] & 0x3f) | 0x80;
    let hex: String = h.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn offline_player(name: &str) -> Result<Player, String> {
    validate_player(name)?;
    Ok(Player {
        name: name.to_string(),
        uuid: offline_uuid(name),
        token: "0".to_string(),
        user_type: "legacy".to_string(),
    })
}

// ───────────────────────────── Progreso hacia la UI ──────────────────────────

#[derive(Serialize, Clone)]
struct Progress {
    stage: &'static str,
    done: usize,
    total: usize,
    instance: Option<String>,
}

/// Evento `launch-progress` → { stage, done, total, instance }.
fn progress(
    app: &AppHandle,
    stage: &'static str,
    done: usize,
    total: usize,
    instance: Option<&str>,
) {
    let _ = app.emit(
        "launch-progress",
        Progress {
            stage,
            done,
            total,
            instance: instance.map(String::from),
        },
    );
}

/// Evento `minecraft-closed` → resultado del proceso y final del log.
#[derive(Serialize, Clone)]
struct CloseInfo {
    code: Option<i32>,
    success: bool,
    log: String,
    instance: Option<String>,
}

/// Últimas líneas del log para mostrar un error útil sin volcar el archivo entero.
fn log_tail(path: &Path) -> String {
    const MAX_BYTES: u64 = 12_000;
    const MAX_LINES: usize = 20;
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(size.saturating_sub(MAX_BYTES)));
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return String::new();
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(MAX_LINES)..].join("\n")
}

fn close_info(status: ExitStatus, log_path: &Path) -> CloseInfo {
    CloseInfo {
        code: status.code(),
        success: status.success(),
        log: log_tail(log_path),
        instance: None,
    }
}

fn startup_error(status: ExitStatus, log_path: &Path) -> String {
    let code = status
        .code()
        .map(|c| format!(" (código {c})"))
        .unwrap_or_default();
    let log = log_tail(log_path);
    if log.is_empty() {
        format!(
            "Minecraft se cerró inesperadamente durante el arranque{code}. No hay salida en el log."
        )
    } else {
        format!("Minecraft se cerró inesperadamente durante el arranque{code}.\n\n{log}")
    }
}

// ─────────────────────────────────── HTTP ────────────────────────────────────

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())
}

pub(crate) fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

#[derive(Clone)]
struct Job {
    url: String,
    dest: PathBuf,
    sha1: Option<String>,
    size: Option<u64>,
}

fn is_up_to_date(job: &Job) -> bool {
    let Ok(meta) = fs::metadata(&job.dest) else {
        return false;
    };
    if meta.len() == 0 {
        return false;
    }
    if let Some(size) = job.size {
        if meta.len() != size {
            return false;
        }
        // Mismo tamaño que el publicado por Mojang: se da por válido sin releerlo entero.
        // Los archivos ya se verificaron con SHA1 al bajarlos y se escriben de forma atómica,
        // así que hashear miles de assets en cada arranque solo hacía lento el JUGAR.
        return true;
    }
    if let Some(expected) = &job.sha1 {
        return fs::read(&job.dest)
            .map(|b| sha1_hex(&b).eq_ignore_ascii_case(expected))
            .unwrap_or(false);
    }
    // Sin hash no se puede garantizar integridad: forzar re-descarga.
    false
}

fn download(c: &reqwest::blocking::Client, job: &Job) -> Result<(), String> {
    if is_up_to_date(job) {
        return Ok(());
    }
    if let Some(parent) = job.dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut last = String::new();
    for _ in 0..3 {
        let res = c
            .get(&job.url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.bytes());
        match res {
            Ok(bytes) => {
                if let Some(expected) = &job.sha1
                    && !sha1_hex(&bytes).eq_ignore_ascii_case(expected)
                {
                    last = format!("SHA1 incorrecto al descargar {}", job.url);
                    continue;
                }
                // Se escribe a un .part y se renombra: nunca queda un archivo a medias.
                let mut tmp = job.dest.clone().into_os_string();
                tmp.push(".part");
                let tmp = PathBuf::from(tmp);
                fs::write(&tmp, &bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
                // Reemplazo seguro compatible con Windows: nunca se borra el archivo
                // válido antes de confirmar que el temporal quedó completo.
                let backup = job.dest.with_extension("old");
                if job.dest.exists() {
                    let _ = fs::remove_file(&backup);
                    fs::rename(&job.dest, &backup)
                        .map_err(|e| format!("{}: {e}", job.dest.display()))?;
                }
                if let Err(e) = fs::rename(&tmp, &job.dest) {
                    // Restaurar el original si el reemplazo falla.
                    if backup.exists() {
                        let _ = fs::rename(&backup, &job.dest);
                    }
                    return Err(format!("{}: {e}", job.dest.display()));
                }
                let _ = fs::remove_file(&backup);
                let _ = fs::remove_file(&tmp);
                return Ok(());
            }
            Err(e) => last = format!("{}: {e}", job.url),
        }
    }
    Err(last)
}

fn get_json(c: &reqwest::blocking::Client, url: &str) -> Result<Value, String> {
    c.get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json::<Value>())
        .map_err(|e| format!("{url}: {e}"))
}

/// Descarga en paralelo. Llama a `on_progress(hechos, total)` a medida que avanza.
fn download_all<F>(
    c: &reqwest::blocking::Client,
    jobs: Vec<Job>,
    on_progress: F,
) -> Result<(), String>
where
    F: Fn(usize, usize) + Sync,
{
    let total = jobs.len();
    if total == 0 {
        return Ok(());
    }
    let queue = Mutex::new(jobs.into_iter());
    let done = AtomicUsize::new(0);
    let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let workers = DOWNLOAD_THREADS.min(total);

    thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let next = queue.lock().unwrap().next();
                    let Some(job) = next else { break };
                    if let Err(e) = download(c, &job) {
                        errors.lock().unwrap().push(e);
                    }
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if n.is_multiple_of(25) || n == total {
                        on_progress(n, total);
                    }
                }
            });
        }
    });

    let errs = errors.into_inner().unwrap();
    if errs.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Fallaron {} descargas. La primera: {}",
            errs.len(),
            errs[0]
        ))
    }
}

// ─────────────────────────────────── Reglas ──────────────────────────────────

fn os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

fn arch_name() -> &'static str {
    if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// Sin reglas = permitido. Con reglas gana la última que aplique.
/// No activamos ninguna "feature" (demo, resolución propia, quick play…).
fn rules_allow(rules: Option<&Value>) -> bool {
    let Some(arr) = rules.and_then(|r| r.as_array()) else {
        return true;
    };
    if arr.is_empty() {
        return true;
    }
    let mut allowed = false;
    for r in arr {
        if let Some(os) = r.get("os") {
            if let Some(n) = os.get("name").and_then(Value::as_str)
                && n != os_name()
            {
                continue;
            }
            if let Some(a) = os.get("arch").and_then(Value::as_str)
                && a != arch_name()
            {
                continue;
            }
        }
        if let Some(f) = r.get("features").and_then(Value::as_object)
            && f.values().any(|v| v.as_bool() == Some(true))
        {
            continue;
        }
        allowed = r.get("action").and_then(Value::as_str) != Some("disallow");
    }
    allowed
}

/// "g.r.p:artifact:1.0[:clasificador]" → "g/r/p/artifact/1.0/artifact-1.0[-clasificador].jar"
pub fn maven_path(name: &str) -> String {
    let name = name.split('@').next().unwrap_or(name);
    let p: Vec<&str> = name.split(':').collect();
    if p.len() < 3 {
        return name.replace(':', "/");
    }
    let file = match p.get(3) {
        Some(c) => format!("{}-{}-{}.jar", p[1], p[2], c),
        None => format!("{}-{}.jar", p[1], p[2]),
    };
    format!("{}/{}/{}/{}", p[0].replace('.', "/"), p[1], p[2], file)
}

// ───────────────────────────── Versión (con herencia) ────────────────────────

fn read_json(path: &Path) -> Result<Value, String> {
    let s = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("JSON inválido en {}: {e}", path.display()))
}

/// Mezcla el JSON del padre dentro del hijo (Fabric/Forge → vanilla).
fn merge_parent(child: &mut Value, parent: &Value) {
    // Librerías: primero las del hijo (tienen prioridad al deduplicar).
    let mut libs = child["libraries"].as_array().cloned().unwrap_or_default();
    libs.extend(parent["libraries"].as_array().cloned().unwrap_or_default());
    child["libraries"] = Value::Array(libs);

    // Argumentos: primero los del padre, después los del hijo.
    for key in ["game", "jvm"] {
        let mut list = parent["arguments"][key]
            .as_array()
            .cloned()
            .unwrap_or_default();
        list.extend(
            child["arguments"][key]
                .as_array()
                .cloned()
                .unwrap_or_default(),
        );
        if !list.is_empty() {
            child["arguments"][key] = Value::Array(list);
        }
    }

    // Lo demás se hereda solo si el hijo no lo define.
    for key in [
        "assetIndex",
        "assets",
        "downloads",
        "javaVersion",
        "minecraftArguments",
    ] {
        if child[key].is_null() && !parent[key].is_null() {
            child[key] = parent[key].clone();
        }
    }
}

/// Carga `versions/<id>/<id>.json` (lo baja de Mojang si falta). Devuelve el JSON ya
/// mezclado y el id de la versión "raíz" (la que aporta el jar del cliente).
fn load_version(
    c: &reqwest::blocking::Client,
    mc: &Path,
    id: &str,
    depth: u8,
) -> Result<(Value, String), String> {
    if depth > 5 {
        return Err("La herencia de versiones es demasiado profunda".into());
    }
    let jpath = mc.join("versions").join(id).join(format!("{id}.json"));
    if !jpath.is_file() {
        let manifest = get_json(c, MANIFEST)?;
        let entry = manifest["versions"]
            .as_array()
            .and_then(|a| a.iter().find(|v| v["id"].as_str() == Some(id)))
            .ok_or_else(|| format!("La versión '{id}' no existe en el manifiesto de Mojang"))?;
        let url = entry["url"]
            .as_str()
            .ok_or("El manifiesto no trae la URL de la versión")?;
        download(
            c,
            &Job {
                url: url.to_string(),
                dest: jpath.clone(),
                sha1: entry["sha1"].as_str().map(String::from),
                size: None,
            },
        )?;
    }
    let mut v = read_json(&jpath)?;
    let parent = v["inheritsFrom"].as_str().map(String::from);
    match parent {
        None => Ok((v, id.to_string())),
        Some(pid) => {
            let (pv, root) = load_version(c, mc, &pid, depth + 1)?;
            merge_parent(&mut v, &pv);
            Ok((v, root))
        }
    }
}

// ─────────────────────────────── Argumentos ──────────────────────────────────

fn expand(arg: &str, vars: &HashMap<&'static str, String>) -> Option<String> {
    let mut s = arg.to_string();
    for (k, v) in vars {
        s = s.replace(&format!("${{{k}}}"), v);
    }
    // Si quedó un ${...} sin resolver, el argumento se descarta.
    if s.contains("${") { None } else { Some(s) }
}

fn collect_args(list: &Value, vars: &HashMap<&'static str, String>, out: &mut Vec<String>) {
    let Some(arr) = list.as_array() else { return };
    for item in arr {
        if let Some(s) = item.as_str() {
            if let Some(e) = expand(s, vars) {
                out.push(e);
            }
            continue;
        }
        if !rules_allow(item.get("rules")) {
            continue;
        }
        match &item["value"] {
            Value::String(s) => {
                if let Some(e) = expand(s, vars) {
                    out.push(e);
                }
            }
            Value::Array(vs) => {
                for v in vs {
                    if let Some(e) = v.as_str().and_then(|s| expand(s, vars)) {
                        out.push(e);
                    }
                }
            }
            _ => {}
        }
    }
}

/// "2G", "2048M", "2048" → "2G" / "2048M". Cualquier cosa rara → "2G".
fn norm_ram(r: Option<String>) -> String {
    let s = r.unwrap_or_default().trim().to_uppercase();
    let cut = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let (num, unit) = s.split_at(cut);
    match (num.parse::<u32>(), unit) {
        (Ok(n), "G") | (Ok(n), "GB") if n > 0 => format!("{n}G"),
        (Ok(n), "M") | (Ok(n), "MB") if n > 0 => format!("{n}M"),
        (Ok(n), "") if n > 0 && n <= 64 => format!("{n}G"),
        (Ok(n), "") if n > 0 => format!("{n}M"),
        _ => "2G".to_string(),
    }
}

// ───────────────────────────────── Java ──────────────────────────────────────

/// Versión mayor de un Java ("1.8.0_402" → 8, "21.0.3" → 21).
pub(crate) fn java_major(java: &str) -> Option<u32> {
    let out = crate::new_cmd(java).arg("-version").output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout)
    );
    let start = text.find('"')? + 1;
    let rest = &text[start..];
    let end = rest.find('"')?;
    let mut parts = rest[..end].split(['.', '_', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

fn runtime_platform() -> Option<&'static str> {
    if !cfg!(windows) {
        return None;
    }
    Some(match arch_name() {
        "x86" => "windows-x86",
        "arm64" => "windows-arm64",
        _ => "windows-x64",
    })
}

/// Java que pide la versión (p. ej. "jre-legacy" = Java 8, "java-runtime-delta" = Java 21):
/// si no está en `.minecraft/runtime`, se baja de Mojang con verificación SHA1.
/// Devuelve la ruta a java.exe, o None si esta plataforma no se soporta.
fn ensure_runtime(
    app: &AppHandle,
    c: &reqwest::blocking::Client,
    mc: &Path,
    component: &str,
) -> Result<Option<String>, String> {
    // Igual que el launcher oficial: runtime/<componente>/<plataforma>/<componente>/bin/java(.exe).
    let Some(platform) = runtime_platform() else {
        return Ok(None);
    };
    if component.is_empty()
        || !component
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(format!("Nombre de runtime inválido: {component}"));
    }
    let base = mc.join("runtime").join(component).join(platform);
    let root = base.join(component);
    let java = root.join("bin").join("java.exe");
    let partial = base.join(".obsidia-partial");

    // Ya instalado (por Obsidia o por el launcher oficial) y sin descarga a medias.
    if java.is_file() && !partial.exists() {
        return Ok(Some(java.to_string_lossy().to_string()));
    }

    let all = get_json(c, RUNTIME_ALL)?;
    let url = all[platform][component][0]["manifest"]["url"]
        .as_str()
        .ok_or_else(|| format!("Mojang no ofrece el Java '{component}' para {platform}"))?
        .to_string();
    let manifest = get_json(c, &url)?;

    fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    fs::write(&partial, b"").map_err(|e| format!("{}: {e}", partial.display()))?;

    let mut jobs: Vec<Job> = Vec::new();
    if let Some(files) = manifest["files"].as_object() {
        for (rel, f) in files {
            // Rutas del servidor: se rechazan las absolutas o con "..".
            if rel.is_empty()
                || rel.contains('\\')
                || rel.contains(':')
                || rel.split('/').any(|p| p.is_empty() || p == "..")
            {
                continue;
            }
            match f["type"].as_str() {
                Some("directory") => {
                    let _ = fs::create_dir_all(root.join(rel));
                }
                Some("file") => {
                    let raw = &f["downloads"]["raw"];
                    let Some(u) = raw["url"].as_str() else {
                        continue;
                    };
                    jobs.push(Job {
                        url: u.to_string(),
                        dest: root.join(rel),
                        sha1: raw["sha1"].as_str().map(String::from),
                        size: raw["size"].as_u64(),
                    });
                }
                _ => {}
            }
        }
    }
    download_all(c, jobs, |d, t| progress(app, "java", d, t, None))?;
    let _ = fs::remove_file(&partial);

    if java.is_file() {
        Ok(Some(java.to_string_lossy().to_string()))
    } else {
        Err("El Java se descargó pero no encuentro java.exe".to_string())
    }
}

/// Comprueba que el Java elegido alcance la versión mayor que pide Minecraft.
fn ensure_java_level(java: String, required: Option<u32>) -> Result<String, String> {
    if let (Some(req), Some(have)) = (required, java_major(&java))
        && have < req
    {
        return Err(format!(
            "Esta versión necesita Java {req} o superior y '{java}' es Java {have}. \
                 Elegí otro Java en Ajustes."
        ));
    }
    Ok(java)
}

/// Java para lanzar una versión: primero el runtime de Mojang que pide esa versión y, si no se
/// puede preparar, un Java instalado compatible. Nunca devuelve el comando `"java"` a ciegas.
fn prepare_java(
    app: &AppHandle,
    c: &reqwest::blocking::Client,
    mc: &Path,
    v: &Value,
    fallback: impl FnOnce(Option<u32>) -> Option<String>,
) -> Result<String, String> {
    let required = v["javaVersion"]["majorVersion"].as_u64().map(|n| n as u32);
    let component = v["javaVersion"]["component"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let runtime_error = match component {
        Some(comp) => match ensure_runtime(app, c, mc, comp) {
            Ok(Some(java)) => return ensure_java_level(java, required),
            Ok(None) => "Mojang no ofrece un runtime para esta plataforma".to_string(),
            Err(e) => e,
        },
        None => "La versión no indica qué runtime de Java usar".to_string(),
    };

    if let Some(java) = fallback(required) {
        return ensure_java_level(java, required);
    }
    let need = required
        .map(|n| format!("Java {n} o superior"))
        .unwrap_or_else(|| "un Java compatible".to_string());
    Err(format!(
        "No pude preparar el Java de Minecraft: {runtime_error}. \
         Tampoco encontré {need} instalado."
    ))
}

/// Resuelve el Java de una versión sin abrir el juego. Lo usan los instaladores de Fabric y
/// Forge para no depender de un Java del sistema.
pub fn java_for_version(
    app: &AppHandle,
    mc: &Path,
    version: &str,
    fallback: impl FnOnce(Option<u32>) -> Option<String>,
) -> Result<String, String> {
    let c = client()?;
    let (v, _) = load_version(&c, mc, version, 0)?;
    prepare_java(app, &c, mc, &v, fallback)
}

// ───────────────────────────────── Natives ───────────────────────────────────

fn extract_natives(jar: &Path, dest: &Path) -> Result<(), String> {
    let f = fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(f)
        .map_err(|e| format!("{} no es un jar válido: {e}", jar.display()))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        let lower = name.to_lowercase();
        let is_native =
            lower.ends_with(".dll") || lower.ends_with(".so") || lower.ends_with(".dylib");
        if entry.is_dir() || name.starts_with("META-INF") || !is_native {
            continue;
        }
        // Solo el nombre del archivo: evita rutas con ".." dentro del zip.
        let Some(file_name) = Path::new(&name).file_name() else {
            continue;
        };
        let target = dest.join(file_name);
        if fs::metadata(&target)
            .map(|m| m.len() == entry.size())
            .unwrap_or(false)
        {
            continue; // ya está extraída
        }
        let mut out =
            fs::File::create(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ─────────────────────────────── Servidores ──────────────────────────────────

/// Separa `host`, `host:puerto` o `[ipv6]:puerto`. Puerto por defecto: 25565.
fn split_server(server: &str) -> Result<(String, u16), String> {
    let s = server.trim();
    if s.is_empty() {
        return Err("La dirección del servidor está vacía".to_string());
    }
    let (host, port) = if let Some(rest) = s.strip_prefix('[') {
        let end = rest.find(']').ok_or("Dirección IPv6 inválida")?;
        let host = &rest[..end];
        let tail = &rest[end + 1..];
        let port = match tail.strip_prefix(':') {
            Some(p) => p,
            None if tail.is_empty() => "25565",
            None => return Err(format!("Dirección de servidor inválida: {server}")),
        };
        (host, port)
    } else {
        match s.rsplit_once(':') {
            Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (h, p),
            Some(_) => return Err(format!("Puerto inválido en {server}")),
            None => (s, "25565"),
        }
    };
    if host.is_empty()
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'))
    {
        return Err(format!("Dirección de servidor inválida: {server}"));
    }
    let port: u16 = port
        .parse()
        .map_err(|_| format!("Puerto inválido: {port}"))?;
    if port == 0 {
        return Err("Puerto inválido: 0".to_string());
    }
    Ok((host.to_string(), port))
}

/// ¿La versión soporta `--quickPlayMultiplayer`? Se agregó en 1.20.
fn supports_quick_play(version: &str) -> bool {
    let mut parts = version.split('.');
    let Some(major) = parts.next().and_then(|p| p.parse::<u32>().ok()) else {
        return true; // snapshot o versión rara: asumimos moderna
    };
    let minor = parts
        .next()
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(0);
    (major, minor) >= (1, 20)
}

/// Argumentos para entrar directo a un servidor. En 1.20+ usa quick play;
/// en versiones viejas `--server`/`--port`.
pub fn server_args(server: &str, mc_version: &str) -> Result<Vec<String>, String> {
    let (host, port) = split_server(server)?;
    if supports_quick_play(mc_version) {
        let addr = if host.contains(':') {
            format!("[{host}]:{port}")
        } else {
            format!("{host}:{port}")
        };
        Ok(vec!["--quickPlayMultiplayer".to_string(), addr])
    } else {
        Ok(vec![
            "--server".to_string(),
            host,
            "--port".to_string(),
            port.to_string(),
        ])
    }
}

// ─────────────────────────────── Lanzamiento ─────────────────────────────────

/// Lo que el perfil (o el botón "Unirse") le pide al lanzamiento.
pub struct LaunchOptions {
    /// Memoria máxima: "4G", "3072M"… Si falta o es rara se usa 2G.
    pub ram: Option<String>,
    /// Java elegido por el usuario. Vacío o `None` = automático.
    pub java_path: Option<String>,
    /// Servidor al que entrar directo (`host` o `host:puerto`).
    pub server: Option<String>,
    pub fullscreen: bool,
}

/// Comando final listo para ejecutar Minecraft.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCommand {
    pub java: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// Construye el comando de lanzamiento a partir del directorio de la instancia.
///
/// Es una función pura: no descarga ni ejecuta nada. Permite verificar que
/// `--gameDir` y `cwd` correspondan al directorio de la instancia correcta.
pub fn build_command(java: &str, args: Vec<String>, game_dir: &Path) -> BuildCommand {
    let mut args = args;
    // Asegurar que --gameDir apunte al directorio de la instancia.
    if !args.iter().any(|a| a.starts_with("--gameDir")) {
        args.push(format!("--gameDir {}", game_dir.display()));
    }
    BuildCommand {
        java: java.to_string(),
        args,
        cwd: game_dir.to_path_buf(),
    }
}
#[allow(clippy::too_many_arguments)]
pub fn launch(
    app: &AppHandle,
    mc: &Path,
    // Carpeta de juego de la instancia (mods, config, mundos, options.txt). `mc` queda para lo compartido.
    game_dir: &Path,
    version: &str,
    player: Player,
    opts: LaunchOptions,
    // Se llama solo si falla el runtime de Mojang: buscar Javas instalados lanza `java -version`.
    fallback_java: impl FnOnce(Option<u32>) -> Option<String>,
    // ID de la instancia (para eventos y log).
    instance_id: Option<String>,
    // Callback que se ejecuta al terminar el proceso con el PID.
    on_close: Option<Box<dyn FnOnce(u32) + Send + 'static>>,
    // Callback que se ejecuta justo después de spawn() con el PID real.
    on_spawn: Option<Box<dyn FnOnce(u32) + Send + 'static>>,
) -> Result<String, String> {
    let LaunchOptions {
        ram,
        java_path,
        server,
        fullscreen,
    } = opts;
    let c = client()?;
    fs::create_dir_all(game_dir).map_err(|e| format!("{}: {e}", game_dir.display()))?;

    // 1) Versión
    progress(app, "version", 0, 1, instance_id.as_deref());
    let (v, root_id) = load_version(&c, mc, version, 0)?;
    progress(app, "version", 1, 1, instance_id.as_deref());

    // 2) Java: se comprueba antes de bajar librerías o assets, para fallar rápido.
    // Prioridad: Java elegido por el usuario > runtime de Mojang de la versión > Java compatible.
    let required = v["javaVersion"]["majorVersion"].as_u64().map(|n| n as u32);
    let java = match java_path.filter(|p| !p.trim().is_empty()) {
        Some(j) => ensure_java_level(j, required)?,
        None => prepare_java(app, &c, mc, &v, fallback_java)?,
    };

    let libs_dir = mc.join("libraries");
    let natives_dir = mc.join("versions").join(version).join("natives");
    fs::create_dir_all(&natives_dir).map_err(|e| format!("{}: {e}", natives_dir.display()))?;

    // 3) Librerías + jar del cliente
    let mut jobs: Vec<Job> = Vec::new();
    let mut classpath: Vec<PathBuf> = Vec::new();
    let mut native_jars: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    let libs = v["libraries"].as_array().cloned().unwrap_or_default();
    for lib in &libs {
        if !rules_allow(lib.get("rules")) {
            continue;
        }
        let name = lib["name"].as_str().unwrap_or("");
        let parts: Vec<&str> = name.split(':').collect();
        if parts.len() < 3 {
            continue;
        }
        let classifier_in_name = parts.get(3).copied().unwrap_or("");
        if classifier_in_name.ends_with("-arm64") && arch_name() != "arm64" {
            continue;
        }
        if classifier_in_name.ends_with("-x86") && arch_name() != "x86" {
            continue;
        }
        let base = lib["url"]
            .as_str()
            .unwrap_or(LIB_BASE)
            .trim_end_matches('/')
            .to_string();

        // Natives "viejas": un clasificador aparte que se extrae (no va al classpath).
        if let Some(map) = lib["natives"].as_object()
            && let Some(cl) = map.get(os_name()).and_then(Value::as_str)
        {
            let cl = cl.replace("${arch}", if arch_name() == "x86" { "32" } else { "64" });
            let art = &lib["downloads"]["classifiers"][cl.as_str()];
            let rel = art["path"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| maven_path(&format!("{name}:{cl}")));
            let dest = libs_dir.join(&rel);
            if !native_jars.contains(&dest) {
                jobs.push(Job {
                    url: art["url"]
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| format!("{base}/{rel}")),
                    dest: dest.clone(),
                    sha1: art["sha1"].as_str().map(String::from),
                    size: art["size"].as_u64(),
                });
                native_jars.push(dest);
            }
        }

        // Jar principal.
        let art = &lib["downloads"]["artifact"];
        // Mismo grupo:artefacto(:clasificador) → gana el primero (el del hijo). Mojang repite
        // a veces la misma librería en dos entradas (una con `natives`): por eso el filtro va
        // solo acá y no corta el procesamiento de las natives de arriba.
        let key = format!("{}:{}:{}", parts[0], parts[1], classifier_in_name);
        if (art.is_object() || lib.get("natives").is_none()) && seen.insert(key) {
            let rel = art["path"]
                .as_str()
                .map(String::from)
                .unwrap_or_else(|| maven_path(name));
            let dest = libs_dir.join(&rel);
            // url vacía = el archivo lo genera el instalador (Forge): no se baja.
            let url = match art["url"].as_str() {
                Some(u) if !u.is_empty() => Some(u.to_string()),
                Some(_) => None,
                None => Some(format!("{base}/{rel}")),
            };
            if let Some(url) = url {
                jobs.push(Job {
                    url,
                    dest: dest.clone(),
                    sha1: art["sha1"].as_str().map(String::from),
                    size: art["size"].as_u64(),
                });
            }
            // LWJGL 3: las natives vienen como jar con ":natives-…" en el nombre.
            if name.contains(":natives-") && !native_jars.contains(&dest) {
                native_jars.push(dest.clone());
            }
            classpath.push(dest);
        }
    }

    // Jar del cliente (el de la versión raíz).
    let client_jar = mc
        .join("versions")
        .join(&root_id)
        .join(format!("{root_id}.jar"));
    let dl_client = &v["downloads"]["client"];
    if let Some(url) = dl_client["url"].as_str() {
        jobs.push(Job {
            url: url.to_string(),
            dest: client_jar.clone(),
            sha1: dl_client["sha1"].as_str().map(String::from),
            size: dl_client["size"].as_u64(),
        });
    }
    classpath.push(client_jar);

    download_all(&c, jobs, |d, t| {
        progress(app, "libraries", d, t, instance_id.as_deref())
    })?;

    let missing: Vec<String> = classpath
        .iter()
        .filter(|p| !p.is_file())
        .map(|p| p.display().to_string())
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "Faltan {} archivos y no se pueden descargar (¿Forge/Fabric sin instalar?):\n  {}",
            missing.len(),
            missing
                .iter()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        ));
    }

    // 4) Assets (sonidos, idiomas…): el índice y después cada objeto, en paralelo.
    let asset_id = v["assetIndex"]["id"]
        .as_str()
        .or(v["assets"].as_str())
        .unwrap_or("legacy")
        .to_string();
    let assets_root = mc.join("assets");
    let idx_path = assets_root.join("indexes").join(format!("{asset_id}.json"));
    if let Some(url) = v["assetIndex"]["url"].as_str() {
        download(
            &c,
            &Job {
                url: url.to_string(),
                dest: idx_path.clone(),
                sha1: v["assetIndex"]["sha1"].as_str().map(String::from),
                size: v["assetIndex"]["size"].as_u64(),
            },
        )?;
    }
    let idx = read_json(&idx_path)?;
    let mut asset_jobs: Vec<Job> = Vec::new();
    let mut hashes: HashSet<String> = HashSet::new();
    if let Some(objs) = idx["objects"].as_object() {
        for o in objs.values() {
            let Some(h) = o["hash"].as_str() else {
                continue;
            };
            if h.len() < 2 || !hashes.insert(h.to_string()) {
                continue;
            }
            let prefix = &h[..2];
            asset_jobs.push(Job {
                url: format!("{ASSET_BASE}/{prefix}/{h}"),
                dest: assets_root.join("objects").join(prefix).join(h),
                sha1: Some(h.to_string()),
                size: o["size"].as_u64(),
            });
        }
    }
    download_all(&c, asset_jobs, |d, t| {
        progress(app, "assets", d, t, instance_id.as_deref())
    })?;

    // 5) Natives
    for jar in &native_jars {
        if jar.is_file() {
            extract_natives(jar, &natives_dir)?;
        }
    }

    // 6) Argumentos
    fn s(p: &Path) -> String {
        p.to_string_lossy().to_string()
    }
    let cp_string = classpath
        .iter()
        .map(|p| s(p.as_path()))
        .collect::<Vec<_>>()
        .join(CP_SEP);

    let mut vars: HashMap<&'static str, String> = HashMap::new();
    vars.insert("auth_player_name", player.name.clone());
    vars.insert("auth_uuid", player.uuid.clone());
    vars.insert("auth_access_token", player.token.clone());
    vars.insert("auth_session", player.token.clone());
    vars.insert("user_type", player.user_type.clone());
    vars.insert("user_properties", "{}".into());
    vars.insert("clientid", String::new());
    vars.insert("auth_xuid", String::new());
    vars.insert("version_name", version.to_string());
    vars.insert(
        "version_type",
        v["type"].as_str().unwrap_or("release").to_string(),
    );
    vars.insert("game_directory", s(game_dir));
    vars.insert("assets_root", s(&assets_root));
    vars.insert(
        "game_assets",
        s(&assets_root.join("virtual").join("legacy")),
    );
    vars.insert("assets_index_name", asset_id);
    vars.insert("natives_directory", s(&natives_dir));
    vars.insert("library_directory", s(&libs_dir));
    vars.insert("classpath_separator", CP_SEP.to_string());
    vars.insert("launcher_name", "Obsidia".into());
    vars.insert("launcher_version", env!("CARGO_PKG_VERSION").into());
    vars.insert("classpath", cp_string.clone());

    let mut args: Vec<String> = vec![format!("-Xmx{}", norm_ram(ram))];
    if v["arguments"]["jvm"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false)
    {
        collect_args(&v["arguments"]["jvm"], &vars, &mut args);
    } else {
        // Versiones viejas (≤1.12) no traen argumentos de JVM.
        args.push(format!("-Djava.library.path={}", s(&natives_dir)));
        args.push("-cp".into());
        args.push(cp_string);
    }
    args.push(
        v["mainClass"]
            .as_str()
            .ok_or("La versión no tiene mainClass")?
            .to_string(),
    );
    if v["arguments"]["game"].is_array() {
        collect_args(&v["arguments"]["game"], &vars, &mut args);
    } else if let Some(legacy) = v["minecraftArguments"].as_str() {
        for tok in legacy.split_whitespace() {
            if let Some(e) = expand(tok, &vars) {
                args.push(e);
            }
        }
    }

    // Pantalla completa por instancia: sincronizar options.txt.
    let options_path = game_dir.join("options.txt");
    let current = options::read_options(&options_path)?;
    let next = options::set_entry(
        &current,
        "fullscreen:",
        if fullscreen { "true" } else { "false" },
    );
    if next != current {
        options::write_options(&options_path, &next)?;
    }
    if fullscreen {
        args.push("--fullscreen".to_string());
    }

    // Entrar directo al servidor elegido (si lo hay).
    if let Some(server) = server.as_deref().filter(|s| !s.trim().is_empty()) {
        args.extend(server_args(server, &root_id)?);
    }

    let cmd = build_command(&java, args, game_dir);

    // 7) Abrir el juego, con su salida en un log.
    progress(app, "starting", 0, 1, instance_id.as_deref());
    let log_dir = game_dir.join(".obsidia");
    fs::create_dir_all(&log_dir).map_err(|e| format!("{}: {e}", log_dir.display()))?;
    let log_path = log_dir.join("launch.log");
    let log = fs::File::create(&log_path).map_err(|e| format!("{}: {e}", log_path.display()))?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;

    let mut child = crate::new_cmd(&cmd.java)
        .args(&cmd.args)
        .current_dir(&cmd.cwd)
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .map_err(|e| format!("No pude ejecutar Java ('{java}'): {e}"))?;
    let pid = child.id();

    // Notificar PID real a RunningInstances.
    if let Some(cb) = on_spawn {
        cb(pid);
    }

    // Copy pid for the wait thread before moving child.
    let pid_for_close = pid;

    // Un Java incompatible o un argumento inválido hace que el proceso muera enseguida.
    // Se comprueba un momento antes de informar que Minecraft inició.
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Err(startup_error(status, &log_path)),
            Ok(None) => {}
            Err(e) => return Err(format!("No pude comprobar el proceso de Minecraft: {e}")),
        }
        if started.elapsed() >= STARTUP_GRACE {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let app2 = app.clone();
    let log_path2 = log_path.clone();
    let instance2 = instance_id.clone();
    std::thread::spawn(move || {
        let info = match child.wait() {
            Ok(status) => close_info(status, &log_path2),
            Err(e) => CloseInfo {
                code: None,
                success: false,
                log: format!("No pude esperar el proceso: {e}"),
                instance: instance2.clone(),
            },
        };
        let mut info = info;
        info.instance = instance2;
        let _ = app2.emit("minecraft-closed", info);
        if let Some(cb) = on_close {
            cb(pid_for_close);
        }
    });

    let instance_for_progress = instance_id.clone();
    progress(app, "starting", 1, 1, instance_for_progress.as_deref());

    Ok(format!(
        "Minecraft {version} iniciado como {} (pid {}). Log: {}",
        player.name,
        pid,
        log_path.display()
    ))
}

// ─────────────────────────────────── Tests ───────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_command_game_dir_y_cwd() {
        let gd1 = std::path::PathBuf::from("C:/inst/A");
        let gd2 = std::path::PathBuf::from("C:/inst/B");
        let c1 = build_command("java", vec!["--version".into()], &gd1);
        let c2 = build_command("java", vec!["--version".into()], &gd2);
        assert_eq!(c1.cwd, gd1);
        assert_eq!(c2.cwd, gd2);
        assert!(
            c1.args
                .iter()
                .any(|a| a.contains("--gameDir") && a.contains("inst/A"))
        );
        assert!(
            c2.args
                .iter()
                .any(|a| a.contains("--gameDir") && a.contains("inst/B"))
        );
        assert_ne!(c1, c2);
    }

    #[test]
    fn running_instances_reserva_y_libera() {
        // Lanzamiento Vanilla: reservas independientes.
        let r = RunningInstances::default();
        assert!(r.reserve("vanilla-1"));
        assert!(!r.reserve("vanilla-1"));
        assert!(r.is_running("vanilla-1"));
        r.set_pid("vanilla-1", 1234);
        // release_if_matches con PID correcto libera.
        assert!(r.release_if_matches("vanilla-1", 1234));
        assert!(!r.is_running("vanilla-1"));
    }

    #[test]
    fn running_instances_pid_real_se_registra() {
        let r = RunningInstances::default();
        assert!(r.reserve("inst-fabric"));
        r.set_pid("inst-fabric", 5678);
        let guard = r.0.lock().unwrap();
        assert_eq!(guard.get("inst-fabric"), Some(&5678));
    }

    #[test]
    fn running_instances_no_libera_otro_pid() {
        let r = RunningInstances::default();
        assert!(r.reserve("inst-forge"));
        r.set_pid("inst-forge", 9999);
        // Intentar liberar con PID incorrecto no libera.
        assert!(!r.release_if_matches("inst-forge", 1111));
        assert!(r.is_running("inst-forge"));
        // Con PID correcto sí libera.
        assert!(r.release_if_matches("inst-forge", 9999));
        assert!(!r.is_running("inst-forge"));
    }

    #[test]
    fn running_instances_dos_instancias_simultaneas() {
        let r = RunningInstances::default();
        assert!(r.reserve("A"));
        assert!(r.reserve("B"));
        r.set_pid("A", 100);
        r.set_pid("B", 200);
        assert!(r.is_running("A"));
        assert!(r.is_running("B"));
        r.release_if_matches("A", 100);
        assert!(!r.is_running("A"));
        assert!(r.is_running("B"));
    }

    #[test]
    fn running_instances_release_incondicional() {
        // Simula error de lanzamiento sin PID real.
        let r = RunningInstances::default();
        assert!(r.reserve("inst-error"));
        assert!(r.is_running("inst-error"));
        r.release("inst-error");
        assert!(!r.is_running("inst-error"));
    }

    #[test]
    fn running_instances_legacy_diferenciado() {
        // Legacy comparte .minecraft: clave diferenciada.
        let r = RunningInstances::default();
        assert!(r.reserve("legacy:.minecraft"));
        assert!(!r.reserve("legacy:.minecraft"));
        assert!(r.is_running("legacy:.minecraft"));
        // Una instancia con ID no colisiona con legacy.
        assert!(r.reserve("pabc123"));
        assert!(r.is_running("pabc123"));
        r.release("legacy:.minecraft");
        assert!(!r.is_running("legacy:.minecraft"));
        assert!(r.is_running("pabc123"));
    }

    #[test]
    fn running_instances_mismo_id_bloquea_ambos_comandos() {
        // launch_instance y launch_version comparten el mismo mapa.
        // El mismo ID no puede reservarse dos veces sin importar el comando.
        let r = RunningInstances::default();
        assert!(r.reserve("inst-A"));
        // Segundo intento (otro comando) bloqueado.
        assert!(!r.reserve("inst-A"));
        r.release("inst-A");
        // Tras liberar, puede reservarse de nuevo.
        assert!(r.reserve("inst-A"));
    }

    #[test]
    fn running_instances_dos_instancias_independientes() {
        let r = RunningInstances::default();
        assert!(r.reserve("inst-1"));
        assert!(r.reserve("inst-2"));
        assert!(r.is_running("inst-1"));
        assert!(r.is_running("inst-2"));
        r.set_pid("inst-1", 1111);
        r.set_pid("inst-2", 2222);
        // Cierre de una libera solo la suya.
        r.release_if_matches("inst-1", 1111);
        assert!(!r.is_running("inst-1"));
        assert!(r.is_running("inst-2"));
    }

    use serde_json::json;

    #[test]
    fn servidores_modernos_y_viejos() {
        assert_eq!(
            server_args("mc.ejemplo.net", "1.21.1").unwrap(),
            vec!["--quickPlayMultiplayer", "mc.ejemplo.net:25565"]
        );
        assert_eq!(
            server_args("play.ejemplo.net:25566", "1.21.4").unwrap(),
            vec!["--quickPlayMultiplayer", "play.ejemplo.net:25566"]
        );
        assert_eq!(
            server_args("mc.ejemplo.net", "1.19.2").unwrap(),
            vec!["--server", "mc.ejemplo.net", "--port", "25565"]
        );
        assert_eq!(
            server_args("127.0.0.1:19132", "1.12.2").unwrap(),
            vec!["--server", "127.0.0.1", "--port", "19132"]
        );
        assert!(server_args("", "1.21").is_err());
        assert!(server_args("host con espacio", "1.21").is_err());
        assert!(server_args("host:abc", "1.21").is_err());
        assert_eq!(
            server_args("[::1]:25565", "1.21").unwrap(),
            vec!["--quickPlayMultiplayer", "[::1]:25565"]
        );
    }

    #[test]
    fn uuid_offline_conocido() {
        assert_eq!(
            offline_uuid("Notch"),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
    }

    #[test]
    fn nombres() {
        assert!(validate_player("Lauti_01").is_ok());
        assert!(validate_player("ab").is_err());
        assert!(validate_player("con espacio").is_err());
    }

    #[test]
    fn ruta_maven() {
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.3:natives-windows"),
            "org/lwjgl/lwjgl/3.3.3/lwjgl-3.3.3-natives-windows.jar"
        );
    }

    #[test]
    fn ram() {
        assert_eq!(norm_ram(Some("4g".into())), "4G");
        assert_eq!(norm_ram(Some("3072".into())), "3072M");
        assert_eq!(norm_ram(Some("4".into())), "4G");
        assert_eq!(norm_ram(Some("-Xmx9".into())), "2G");
        assert_eq!(norm_ram(None), "2G");
    }

    #[test]
    fn reglas() {
        let otro = if os_name() == "windows" {
            "osx"
        } else {
            "windows"
        };
        assert!(rules_allow(None));
        assert!(rules_allow(Some(
            &json!([{"action":"allow","os":{"name":os_name()}}])
        )));
        assert!(!rules_allow(Some(
            &json!([{"action":"allow","os":{"name":otro}}])
        )));
        assert!(!rules_allow(Some(
            &json!([{"action":"allow","features":{"is_demo_user":true}}])
        )));
        assert!(rules_allow(Some(&json!([
            {"action":"allow"},
            {"action":"disallow","os":{"name":otro}}
        ]))));
    }

    #[test]
    fn herencia() {
        let mut hijo = json!({
            "id":"fabric","mainClass":"net.fabricmc.Main","inheritsFrom":"1.21.1",
            "libraries":[{"name":"a:b:1"}],
            "arguments":{"game":["--x"],"jvm":["-Dh=1"]}
        });
        let padre = json!({
            "id":"1.21.1","mainClass":"vanilla","assetIndex":{"id":"17"},
            "libraries":[{"name":"c:d:2"}],
            "arguments":{"game":["--y"],"jvm":["-Dp=1"]}
        });
        merge_parent(&mut hijo, &padre);
        assert_eq!(hijo["mainClass"], "net.fabricmc.Main");
        assert_eq!(hijo["assetIndex"]["id"], "17");
        assert_eq!(hijo["libraries"][0]["name"], "a:b:1");
        assert_eq!(hijo["libraries"][1]["name"], "c:d:2");
        assert_eq!(hijo["arguments"]["game"], json!(["--y", "--x"]));
    }

    #[test]
    fn argumentos() {
        let mut vars: HashMap<&'static str, String> = HashMap::new();
        vars.insert("auth_player_name", "Lauti".into());
        let lista = json!([
            "--username", "${auth_player_name}",
            {"rules":[{"action":"allow","features":{"is_demo_user":true}}],"value":"--demo"},
            {"rules":[{"action":"allow"}],"value":["--a","--b"]},
            "${desconocido}"
        ]);
        let mut out = vec![];
        collect_args(&lista, &vars, &mut out);
        assert_eq!(out, vec!["--username", "Lauti", "--a", "--b"]);
    }
}
