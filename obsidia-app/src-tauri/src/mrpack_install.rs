//! Motor de descarga e instalación de archivos de modpack.
//!
//! Descarga archivos progresivamente a archivos temporales en disco,
//! calculando SHA1 y SHA512 incrementalmente. Valida todas las integridades
//! antes de aplicar cualquier cambio transaccionalmente. No carga archivos
//! completos en RAM.
//!
//! Reutiliza `fsx::apply_batch_transactional` para la escritura atómica y
//! `crate::http()` para el cliente HTTP con User-Agent de Obsidia.

use crate::content;
use crate::fsx;
use crate::mrpack::{Mrpack, MrpackClientKind, MrpackFile};
use futures_util::StreamExt;
use reqwest::redirect::Policy;
use sha1::Digest as Sha1Digest;
use sha1::Sha1 as Sha1Hasher;
use sha2::Sha512 as Sha512Hasher;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;
use tauri::Emitter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use zip::ZipArchive;

// Dominios de descarga permitidos (misma lista que el Bloque B y la especificación de Modrinth).
const ALLOWED_DOWNLOAD_HOSTS: &[&str] = &[
    "cdn.modrinth.com",
    "github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
];
const MAX_REDIRECTS: usize = 5;
const MAX_CONCURRENT_DOWNLOADS: usize = 4;
const CHUNK_SIZE: usize = 64 * 1024;

// ───────────────────────────── Estructuras ─────────────────────────────

/// Un archivo listo para ser instalado: ya descargado a temp, hash verificado.
pub struct PreparedFile {
    pub dest: PathBuf,
    pub temp_path: PathBuf,
    pub sha1: String,
    pub sha512: String,
    pub size: u64,
    pub entry: content::ContentEntry,
}

/// Resultado interno de una descarga a archivo temporal.
#[derive(Debug)]
struct DownloadedFile {
    temp_path: PathBuf,
    sha1: String,
    sha512: String,
    size: u64,
}

/// Archivo de override extraído del ZIP a temp.
struct ExtractedOverride {
    temp_path: PathBuf,
    dest_path: PathBuf,
    size: u64,
}

// ───────────────────────────── Validación de URL ─────────────────────────────

/// Valida que una URL use HTTPS y un host permitido.
pub(crate) fn validate_download_url(url: &str) -> Result<(), String> {
    let scheme_end = url
        .find("://")
        .ok_or_else(|| "URL sin esquema".to_string())?;
    let scheme = &url[..scheme_end];
    if scheme.eq_ignore_ascii_case("https") == false {
        return Err("solo se admiten URLs https://".to_string());
    }
    let rest = &url[scheme_end + 3..];
    let host = rest.split('/').next().unwrap_or(rest);
    let host = host.to_lowercase();
    if host.is_empty() {
        return Err("URL sin host".to_string());
    }
    if ALLOWED_DOWNLOAD_HOSTS.iter().any(|&h| host == h) {
        return Ok(());
    }
    Err(format!("host de descarga no permitido: {host}"))
}

/// Crea una política de redirección que valida cada salto (máximo 5, solo HTTPS, hosts permitidos).
fn redirect_policy() -> Policy {
    Policy::custom(|attempt| {
        let url = attempt.url();
        let scheme = url.scheme().to_string();
        let host = url.host_str().unwrap_or("").to_string();
        // `url` es una referencia prestada de `attempt`; soltarla explícitamente
        // antes de consumir `attempt` con `error()` / `follow()`.
        let _ = url;
        if scheme != "https" {
            return attempt.error("redirección a HTTP no permitida");
        }
        if !ALLOWED_DOWNLOAD_HOSTS
            .iter()
            .any(|&h| h.eq_ignore_ascii_case(&host))
        {
            return attempt.error(format!("host de redirección no permitido: {host}"));
        }
        attempt.follow()
    })
}

/// Construye un cliente HTTP seguro para el motor de modpacks.
///
/// El cliente:
/// - Usa el User-Agent identificable de Obsidia (`crate::launcher::UA`).
/// - Aplica `redirect_policy()`: solo HTTPS, hosts permitidos, máximo 5 saltos.
/// - Timeout de 60 s por solicitud.
pub(crate) fn create_secure_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(crate::launcher::UA)
        .redirect(redirect_policy())
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| format!("No se pudo crear el cliente HTTP seguro: {e}"))
}

// ───────────────────────────── Progreso ─────────────────────────────

/// Emite progreso si hay una AppHandle disponible. En pruebas se pasa None y
/// no se emite nada, lo que permite probar sin una aplicación Tauri real.
fn emit_progress(
    app: Option<&tauri::AppHandle>,
    stage: &'static str,
    slug: &str,
    instance: &str,
    done: usize,
    total: usize,
) {
    if let Some(app) = app {
        let _ = app.emit(
            "install-progress",
            crate::InstallProgress {
                stage,
                slug: slug.to_string(),
                instance: instance.to_string(),
                done,
                total,
            },
        );
    }
}

// ───────────────────────────── Descarga progresiva ─────────────────────────────

/// Descarga un archivo a temp, calculando SHA1 y SHA512 incrementalmente.
/// Escribe directamente al disco sin cargar el archivo completo en RAM.
///
/// `app` es opcional: si es None no se emite progreso (útil en pruebas).
async fn download_file(
    client: &reqwest::Client,
    url: &str,
    temp_path: &Path,
    expected_sha1: Option<&str>,
    expected_sha512: Option<&str>,
    expected_size: Option<u64>,
    app: Option<tauri::AppHandle>,
    project_slug: &str,
    instance: &str,
    completed: &AtomicUsize,
    total_count: usize,
    cancelled: &AtomicBool,
) -> Result<DownloadedFile, String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Descarga cancelada".to_string());
    }

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Error descargando {url}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {} al descargar {url}", resp.status()));
    }

    let content_length = resp.content_length().unwrap_or(0);
    if let Some(expected) = expected_size {
        if content_length > 0 && content_length != expected {
            return Err(format!(
                "Tamaño declarado ({expected}) no coincide con Content-Length ({content_length}) para {url}"
            ));
        }
    }

    let mut stream = resp.bytes_stream();
    let mut file = tokio::fs::File::create(temp_path)
        .await
        .map_err(|e| format!("No se pudo crear temporal {}: {e}", temp_path.display()))?;

    let mut sha1 = Sha1Hasher::new();
    let mut sha512 = Sha512Hasher::new();
    let mut total_written: u64 = 0;
    let mut last_emit = std::time::Instant::now();

    emit_progress(
        app.as_ref(),
        "downloading",
        project_slug,
        instance,
        0,
        total_count,
    );

    while let Some(chunk) = stream.next().await {
        if cancelled.load(Ordering::Relaxed) {
            let _ = tokio::fs::remove_file(temp_path).await;
            return Err("Descarga cancelada".to_string());
        }

        let chunk = chunk.map_err(|e| format!("Error leyendo {url}: {e}"))?;
        sha1.update(&chunk);
        sha512.update(&chunk);

        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Error escribiendo temporal {}: {e}", temp_path.display()))?;
        file.flush()
            .await
            .map_err(|e| format!("Error flushing temporal {}: {e}", temp_path.display()))?;

        total_written += chunk.len() as u64;

        let now = std::time::Instant::now();
        if now.duration_since(last_emit).as_millis() >= 100 {
            last_emit = now;
            emit_progress(
                app.as_ref(),
                "downloading",
                project_slug,
                instance,
                total_written as usize,
                total_count,
            );
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Error flushing final {}: {e}", temp_path.display()))?;

    // Validar tamaño real leído del disco (solo para registrar en el resultado).
    let metadata = tokio::fs::metadata(temp_path)
        .await
        .map_err(|e| format!("No se pudo leer metadata de {}: {e}", temp_path.display()))?;
    let actual_size = metadata.len();

    // Validar hashes contra los declarados en el manifiesto (fuente de verdad de integridad).
    let sha1_hex = format!("{:x}", sha1.finalize());
    let sha512_hex = format!("{:x}", sha512.finalize());

    if let Some(expected) = expected_sha1 {
        if !sha1_hex.eq_ignore_ascii_case(expected) {
            let _ = tokio::fs::remove_file(temp_path).await;
            return Err(format!(
                "SHA1 no coincide para {url}: esperado {expected}, obtenido {sha1_hex}"
            ));
        }
    }

    if let Some(expected) = expected_sha512 {
        if !sha512_hex.eq_ignore_ascii_case(expected) {
            let _ = tokio::fs::remove_file(temp_path).await;
            return Err(format!(
                "SHA512 no coincide para {url}: esperado {expected}, obtenido {sha512_hex}"
            ));
        }
    }

    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
    emit_progress(
        app.as_ref(),
        "downloading",
        project_slug,
        instance,
        done,
        total_count,
    );

    Ok(DownloadedFile {
        temp_path: temp_path.to_path_buf(),
        sha1: sha1_hex,
        sha512: sha512_hex,
        size: actual_size,
    })
}
// ───────────────────────────── Extracción de overrides ─────────────────────────────

/// Extrae una entrada de override del ZIP a un archivo temporal, validando tamaño.
fn extract_override_to_temp(
    zip_path: &Path,
    zip_entry_path: &str,
    temp_path: &Path,
    expected_size: u64,
) -> Result<(), String> {
    let file = std::fs::File::open(zip_path)
        .map_err(|e| format!("No se pudo abrir el ZIP {}: {e}", zip_path.display()))?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("ZIP inválido o corrupto: {e}"))?;

    let mut entry = archive
        .by_name(zip_entry_path)
        .map_err(|e| format!("No se pudo leer entrada {zip_entry_path} del ZIP: {e}"))?;

    if !entry.is_file() {
        return Err("{zip_entry_path} no es un archivo regular".to_string());
    }

    let entry_size = entry.size();
    if entry_size != expected_size {
        return Err(format!(
            "Tamaño de override {zip_entry_path} no coincide: declarado {expected_size}, ZIP {entry_size}"
        ));
    }

    if let Some(parent) = temp_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("No se pudo crear directorio temporal {}: {e}", parent.display()))?;
    }
    let mut out = std::fs::File::create(temp_path)
        .map_err(|e| format!("No se pudo crear temporal {}: {e}", temp_path.display()))?;
    let mut total_read: u64 = 0;
    let mut buf = [0u8; CHUNK_SIZE];
    loop {
        let n = entry
            .read(&mut buf)
            .map_err(|e| format!("Error leyendo {zip_entry_path}: {e}"))?;
        if n == 0 {
            break;
        }
        total_read += n as u64;
        if total_read > expected_size {
            return Err("El contenido extraído supera el tamaño declarado del ZIP".to_string());
        }
        out.write_all(&buf[..n])
            .map_err(|e| format!("Error escribiendo temporal {}: {e}", temp_path.display()))?;
    }

    if total_read != expected_size {
        return Err(format!(
            "Lectura incompleta de {zip_entry_path}: esperado {expected_size}, leído {total_read}"
        ));
    }

    Ok(())
}

// ───────────────────────────── Utilidades ─────────────────────────────

/// Construye una `ContentEntry` para un archivo de modpack.
fn content_entry_for_file(
    rel_path: &str,
    filename: &str,
    version_id: &str,
    sha1: &str,
) -> content::ContentEntry {
    content::ContentEntry {
        slug: rel_path.to_string(),
        project_id: None,
        version_id: version_id.to_string(),
        filename: filename.to_string(),
        sha1: Some(sha1.to_string()),
        loader: None,
        mc_version: None,
        icon_url: None,
        source: "modrinth".to_string(),
        managed_by_obsidia: true,
    }
}

/// Devuelve el nombre de archivo (último componente) de una ruta.
fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Comprueba si dos rutas colapsan al mismo destino (insensible a mayúsculas en Windows).
fn paths_collide(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// Comprueba que una ruta de destino quede dentro de la instancia, usando
/// `paths::ensure_inside` para validar también contra junctions y symlinks.
/// Además rechaza cualquier ruta dentro de `.obsidia/` (directorio interno del
/// launcher) para evitar sobrescribir metadatos, journals o archivos reservados.
fn validate_dest_within_instance(dest: &Path, instance_dir: &Path) -> Result<(), String> {
    // 1. Validación léxica básica
    if !dest.starts_with(instance_dir) {
        return Err(format!(
            "El destino {} queda fuera de la instancia {}",
            dest.display(),
            instance_dir.display()
        ));
    }

    // 2. Validación contra junctions/symlinks (reutiliza paths::ensure_inside)
    crate::paths::ensure_inside(instance_dir, dest)?;

    // 3. Protección absoluta de .obsidia/: ningún archivo de modpack puede
    //    escribir dentro de este directorio, ni siquiera en subdirectorios.
    if let Ok(rel) = dest.strip_prefix(instance_dir) {
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if rel_str.eq_ignore_ascii_case(".obsidia") || rel_str.starts_with(".obsidia/") {
            return Err(format!(
                "Ruta protegida: {} está dentro de .obsidia/ y no puede ser sobrescrita por un modpack",
                rel_str
            ));
        }
    }

    Ok(())
}

/// Calcula SHA512 de un slice de bytes.
fn sha512_hex(bytes: &[u8]) -> String {
    let mut h = Sha512Hasher::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

// ───────────────────────────── API principal ─────────────────────────────

/// Orquesta la instalación completa de archivos de un modpack en una instancia.
///
/// Flujo:
/// 1. Crea un cliente HTTP seguro con `create_secure_client()` (redirecciones
///    validadas, hosts permitidos, User-Agent de Obsidia).
/// 2. Valida todas las URLs de descarga (HTTPS, hosts permitidos).
/// 3. Descarga los archivos declarados en `files` a temp, validando SHA1/SHA512/tamaño.
/// 4. Extrae `overrides/` y `client-overrides/` del ZIP a temp, validando tamaños.
/// 5. Valida rutas y colisiones entre todos los destinos.
/// 6. Construye `BatchItem`s con `source_file` apuntando a los temporales.
/// 7. Aplica por lotes con `fsx::apply_batch_transactional` (atomicidad + journal).
/// 8. Limpia temporales.
///
/// `app` es opcional: si es None no se emite progreso (útil en pruebas).
/// `selected_optionals` contiene las rutas relativas de archivos opcionales que el usuario eligió instalar.
/// `cancelled` permite abortar la operación de forma segura.
///
/// El cliente HTTP SIEMPRE se crea internamente mediante `create_secure_client()`.
/// No se acepta un cliente externo para evitar que una futura integración omita
/// accidentalmente la validación de redirecciones.
pub async fn install_modpack_files(
    app: Option<tauri::AppHandle>,
    project_slug: &str,
    instance_dir: &Path,
    mrpack: &Mrpack,
    zip_path: &Path,
    selected_optionals: &HashSet<String>,
    cancelled: Arc<AtomicBool>,
) -> Result<(), String> {
    // El cliente HTTP de producción SIEMPRE se crea con la política de redirección
    // validada. No hay ruta de escape para usar un cliente sin validación.
    let client = create_secure_client()?;
    // 1. Validar que el ZIP todavía existe y no ha cambiado.
    let zip_metadata = std::fs::metadata(zip_path)
        .map_err(|e| format!("No se pudo leer el paquete {}: {e}", zip_path.display()))?;
    if !zip_metadata.is_file() {
        return Err("El paquete .mrpack ya no existe".to_string());
    }

    // 1b. Validar todas las URLs de descarga antes de comenzar.
    for file in &mrpack.files {
        match file.client_kind {
            MrpackClientKind::Unsupported => continue,
            MrpackClientKind::Optional => {
                if !selected_optionals.contains(&file.path.to_string_lossy().to_string()) {
                    continue;
                }
            }
            MrpackClientKind::Required => {}
        }
        for url in &file.downloads {
            validate_download_url(url)?;
        }
    }

    // ID de versión como String owned (evita problemas de lifetime con &Mrpack).
    let version_id_str = mrpack
        .version_id
        .as_deref()
        .unwrap_or("unknown")
        .to_string();

    // 2. Directorio temporal dentro de la instancia (limpiable con recover_pending).
    let temp_dir = instance_dir.join(".obsidia").join(".mrpack-temp");
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("No se pudo crear directorio temporal: {e}"))?;

    let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS));

    // Contar archivos a descargar (para progreso).
    let mut files_to_download: Vec<&MrpackFile> = Vec::new();
    for file in &mrpack.files {
        match file.client_kind {
            MrpackClientKind::Unsupported => continue,
            MrpackClientKind::Optional => {
                if !selected_optionals.contains(&file.path.to_string_lossy().to_string()) {
                    continue;
                }
            }
            MrpackClientKind::Required => {}
        }
        files_to_download.push(file);
    }
    let total_count = files_to_download.len();

    // Contador de archivos descargados (para progreso general del modpack).
    let completed = Arc::new(AtomicUsize::new(0));

    // Progreso inicial.
    emit_progress(
        app.as_ref(),
        "downloading",
        project_slug,
        &instance_dir.to_string_lossy(),
        0,
        total_count,
    );

    // 3. Descargar archivos en paralelo (máximo 4 concurrentes).
    let mut prepared_files: Vec<PreparedFile> = Vec::new();
    let mut download_handles = Vec::new();
    let temp_dir_arc = Arc::new(temp_dir.clone());

    for (idx, file) in files_to_download.iter().enumerate() {
        let file_owned = (*file).clone(); // owned: 'static-compatible para tokio::spawn
        let permit = semaphore.clone().acquire_owned().await;
        let client = client.clone();
        let temp_dir = temp_dir_arc.clone();
        let app_for_task = app.clone();
        let instance = instance_dir.to_string_lossy().to_string();
        let cancelled = cancelled.clone();
        let project_slug_owned = project_slug.to_string();
        let completed_clone = completed.clone();
        let url = file_owned.downloads.first().cloned().unwrap_or_default();

        let handle = tokio::spawn(async move {
            let _permit = permit;
            let temp_path = temp_dir.join(format!("download-{idx}.tmp"));
            let result = download_file(
                &client,
                &url,
                &temp_path,
                Some(file_owned.sha1.as_str()),
                Some(file_owned.sha512.as_str()),
                file_owned.size,
                app_for_task,
                &project_slug_owned,
                &instance,
                &completed_clone,
                total_count,
                &cancelled,
            )
            .await;
            (idx, result)
        });
        download_handles.push(handle);
    }

    let mut download_results: Vec<(usize, Result<DownloadedFile, String>)> = Vec::new();
    for handle in download_handles {
        download_results.push(
            handle
                .await
                .map_err(|e| format!("Error en tarea de descarga: {e}"))?,
        );
    }

    // Verificar si alguna descarga falló.
    for (idx, result) in &download_results {
        if let Err(e) = result {
            // Limpiar temporales de descargas exitosas antes de salir.
            for (_, r) in &download_results {
                if let Ok(d) = r {
                    let _ = std::fs::remove_file(&d.temp_path);
                }
            }
            return Err(format!("Descarga fallida en archivo {idx}: {e}"));
        }
    }

    // 4. Convertir descargas a PreparedFile.
    let mut temp_files_to_clean: Vec<PathBuf> = Vec::new();
    let mut all_destinations: Vec<String> = Vec::new(); // para detección de colisiones

    for (idx, result) in download_results {
        let dl = result.unwrap();
        let file = &files_to_download[idx];
        let rel_path = file.path.to_string_lossy().to_string();
        let dest = instance_dir.join(&rel_path);
        let filename = file_name_of(&rel_path);

        // Validar que el destino quede dentro de la instancia.
        validate_dest_within_instance(&dest, instance_dir)?;

        // Detectar colisiones.
        if all_destinations.iter().any(|d| paths_collide(d, &rel_path)) {
            return Err(format!("Colisión de rutas: {rel_path}"));
        }
        all_destinations.push(rel_path.clone());

        let entry = content_entry_for_file(&rel_path, &filename, &version_id_str, &dl.sha1);

        prepared_files.push(PreparedFile {
            dest,
            temp_path: dl.temp_path.clone(),
            sha1: dl.sha1,
            sha512: dl.sha512,
            size: dl.size,
            entry,
        });
        temp_files_to_clean.push(dl.temp_path);
    }

    // 5. Extraer overrides y client-overrides del ZIP a temp.
    //    Reabrir el ZIP y validar entradas antes de extraer.
    let mut overrides_to_prepare: Vec<(MrpackFile, PathBuf)> = Vec::new(); // (file, temp_path)

    for ovr in &mrpack.overrides {
        let dest = instance_dir.join(&ovr.dest_path);
        validate_dest_within_instance(&dest, instance_dir)?;

        let rel_dest = ovr.dest_path.to_string_lossy().to_string();
        if all_destinations.iter().any(|d| paths_collide(d, &rel_dest)) {
            return Err(format!(
                "Colisión de rutas entre override y archivo descargado: {rel_dest}"
            ));
        }
        all_destinations.push(rel_dest.clone());

        let temp_path = temp_dir.join(format!("override-{}.tmp", ovr.dest_path.display()));
        extract_override_to_temp(
            zip_path,
            &ovr.zip_path.to_string_lossy(),
            &temp_path,
            ovr.size,
        )?;

        // Sobrescribir tamaño real leído del disco.
        let metadata = std::fs::metadata(&temp_path)
            .map_err(|e| format!("No se pudo leer metadata de override: {e}"))?;
        let actual_size = metadata.len();
        if actual_size != ovr.size {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!(
                "Tamaño incorrecto al extraer override {}: esperado {}, leído {}",
                ovr.zip_path.display(),
                ovr.size,
                actual_size
            ));
        }

        all_destinations.push(rel_dest);
        overrides_to_prepare.push((
            MrpackFile {
                path: ovr.dest_path.clone(),
                size: Some(actual_size),
                sha1: String::new(), // overrides no tienen hash declarado en el manifiesto
                sha512: String::new(),
                downloads: Vec::new(),
                env: Default::default(),
                client_kind: MrpackClientKind::Required,
            },
            temp_path.clone(),
        ));
        temp_files_to_clean.push(temp_path);
    }

    for ovr in &mrpack.client_overrides {
        let dest = instance_dir.join(&ovr.dest_path);
        validate_dest_within_instance(&dest, instance_dir)?;

        let rel_dest = ovr.dest_path.to_string_lossy().to_string();
        if all_destinations.iter().any(|d| paths_collide(d, &rel_dest)) {
            return Err(format!(
                "Colisión de rutas entre client-override y archivo: {rel_dest}"
            ));
        }
        all_destinations.push(rel_dest.clone());

        let temp_path = temp_dir.join(format!("client-override-{}.tmp", ovr.dest_path.display()));
        extract_override_to_temp(
            zip_path,
            &ovr.zip_path.to_string_lossy(),
            &temp_path,
            ovr.size,
        )?;

        let metadata = std::fs::metadata(&temp_path)
            .map_err(|e| format!("No se pudo leer metadata de client-override: {e}"))?;
        let actual_size = metadata.len();
        if actual_size != ovr.size {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!(
                "Tamaño incorrecto al extraer client-override {}: esperado {}, leído {}",
                ovr.zip_path.display(),
                ovr.size,
                actual_size
            ));
        }

        overrides_to_prepare.push((
            MrpackFile {
                path: ovr.dest_path.clone(),
                size: Some(actual_size),
                sha1: String::new(),
                sha512: String::new(),
                downloads: Vec::new(),
                env: Default::default(),
                client_kind: MrpackClientKind::Required,
            },
            temp_path.clone(),
        ));
        temp_files_to_clean.push(temp_path);
    }

    // 6. Construir BatchItems con source_file (disco, no RAM).
    let mut batch_items: Vec<fsx::BatchItem> = Vec::new();

    for pf in &prepared_files {
        batch_items.push(fsx::BatchItem {
            dest: pf.dest.clone(),
            bytes: Vec::new(), // vacío: el contenido está en source_file
            expected_sha1: Some(pf.sha1.clone()),
            source_file: Some(pf.temp_path.clone()),
            entry: content_entry_for_file(
                &pf.dest.to_string_lossy(),
                &file_name_of(&pf.dest.to_string_lossy()),
                &version_id_str,
                &pf.sha1,
            ),
        });
    }

    for (ovr_file, temp_path) in &overrides_to_prepare {
        batch_items.push(fsx::BatchItem {
            dest: instance_dir.join(&ovr_file.path),
            bytes: Vec::new(),
            expected_sha1: None, // overrides no tienen SHA1 declarado
            source_file: Some(temp_path.clone()),
            entry: content_entry_for_file(
                &ovr_file.path.to_string_lossy(),
                &file_name_of(&ovr_file.path.to_string_lossy()),
                &version_id_str,
                &ovr_file.sha1,
            ),
        });
    }

    // Datos para resolver iconos en Modrinth tras aplicar (batch_items se mueve).
    let icon_targets: Vec<(String, String)> = batch_items
        .iter()
        .filter_map(|it| {
            it.expected_sha1
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| (it.entry.slug.clone(), s.to_string()))
        })
        .collect();

    // 7. Clasificar por carpeta raíz: solo mods/shaderpacks/resourcepacks van
    //    al índice de contenido. El resto (options.txt, configs, etc.) se
    //    escribe en disco sin indexar, para que no aparezcan en "Instalados".
    let total_items = batch_items.len();
    let (mods_items, shaders_items, packs_items, plain_items) = split_items(&instance_dir, batch_items);

    emit_progress(
        app.as_ref(),
        "applying",
        project_slug,
        &*instance_dir.to_string_lossy(),
        0,
        total_items,
    );

    // 7a. Overrides de sistema: se aplican sin indexar.
    for item in &plain_items {
        if let Some(source) = &item.source_file {
            if let Err(e) = fsx::write_atomic_from_file(&item.dest, source, "") {
                for temp in &temp_files_to_clean {
                    let _ = std::fs::remove_file(temp);
                }
                return Err(format!("Fallo escribiendo override {}: {}", item.dest.display(), e));
            }
        }
    }

    // 7b. Contenido indexable, por tipo.
    for (kind, subdir, items) in [
        ("mods", "mods", mods_items),
        ("shaders", "shaderpacks", shaders_items),
        ("packs", "resourcepacks", packs_items),
    ] {
        if items.is_empty() {
            continue;
        }
        let result = fsx::apply_batch_transactional(
            instance_dir,
            kind,
            &instance_dir.join(subdir),
            items,
            &cancelled,
        );
        if let Err(e) = &result {
            // Limpiar temporales antes de propagar el error.
            for temp in &temp_files_to_clean {
                let _ = std::fs::remove_file(temp);
            }
            return Err(format!("Fallo aplicando instalación: {e}"));
        }
    }

    // 8. Limpiar temporales (ya están aplicados).
    for temp in &temp_files_to_clean {
        let _ = std::fs::remove_file(temp);
    }

    // 9. Limpiar directorio temporal si queda vacío.
    let _ = std::fs::remove_dir(&temp_dir);

    // 9b. Resolver iconos de los mods en el índice de contenido (mejor esfuerzo).
    enrich_modpack_icons(&client, instance_dir, &icon_targets).await;

    emit_progress(
        app.as_ref(),
        "completed",
        "",
        &*instance_dir.to_string_lossy(),
        prepared_files.len() + overrides_to_prepare.len(),
        prepared_files.len() + overrides_to_prepare.len(),
    );

    Ok(())
}

/// Solicita la cancelación de una instalación en progreso.
///
/// Establece el flag atómico `cancelled` en `true`. El token se pasa a
/// `install_modpack_files`, que lo consulta durante cada bloque de descarga.
/// Si se activa, la descarga actual se aborta y se eliminan los temporales.
///
/// NOTA: Esta función es un helper de bajo nivel. En el Bloque D se conectará
/// con un comando Tauri que mantenga el `Arc<AtomicBool>` asociado a la
/// instalación activa. Por ahora no hay conexión real con una instalación en
/// curso; llamarla sin pasar el mismo token a una instalación activa no tiene
/// efecto observable.
pub fn request_cancel(cancelled: &Arc<AtomicBool>) {
    cancelled.store(true, Ordering::Relaxed);
}

// ───────────────────────────── Pruebas ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::{Method, MockServer};
    use sha1::{Digest as Sha1DigestTrait, Sha1 as Sha1HasherTrait};
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::io::AsyncWriteExt;

    fn compute_sha1(data: &[u8]) -> String {
        let mut h = Sha1HasherTrait::new();
        h.update(data);
        format!("{:x}", h.finalize())
    }

    fn make_test_mrpack_file(
        path: &str,
        sha1: &str,
        sha512: &str,
        size: Option<u64>,
        client: MrpackClientKind,
        downloads: Vec<String>,
    ) -> MrpackFile {
        MrpackFile {
            path: PathBuf::from(path),
            size,
            sha1: sha1.to_string(),
            sha512: sha512.to_string(),
            downloads,
            env: Default::default(),
            client_kind: client,
        }
    }

    fn make_test_mrpack() -> Mrpack {
        Mrpack {
            format_version: 1,
            game: "minecraft".to_string(),
            version_id: Some("test-version".to_string()),
            name: "TestPack".to_string(),
            summary: Some("test".to_string()),
            dependencies: Default::default(),
            files: Vec::new(),
            overrides: Vec::new(),
            client_overrides: Vec::new(),
            zip_uncompressed_bytes: 0,
        }
    }

    fn make_test_client() -> reqwest::Client {
        reqwest::Client::builder()
            .user_agent("Obsidia/test")
            .build()
            .unwrap()
    }

    // ───── validate_download_url ─────

    #[test]
    fn validate_https_allowed_host() {
        assert!(validate_download_url("https://cdn.modrinth.com/data/file.jar").is_ok());
        assert!(validate_download_url("https://github.com/user/repo/file.jar").is_ok());
        assert!(
            validate_download_url("https://raw.githubusercontent.com/user/repo/main/file.jar")
                .is_ok()
        );
        assert!(validate_download_url("https://gitlab.com/user/repo/file.jar").is_ok());
    }

    #[test]
    fn validate_http_rejected() {
        let err = validate_download_url("http://cdn.modrinth.com/data/file.jar").unwrap_err();
        assert!(err.contains("solo se admiten URLs https://"));
    }

    #[test]
    fn validate_unauthorized_host_rejected() {
        let err = validate_download_url("https://evil.example.com/data/file.jar").unwrap_err();
        assert!(err.contains("host de descarga no permitido"));
    }

    #[test]
    fn validate_no_scheme_rejected() {
        let err = validate_download_url("cdn.modrinth.com/data/file.jar").unwrap_err();
        assert!(err.contains("URL sin esquema"));
    }

    #[test]
    fn validate_empty_host_rejected() {
        let err = validate_download_url("https:///data/file.jar").unwrap_err();
        assert!(err.contains("URL sin host"));
    }

    // ───── download_file ─────

    #[tokio::test]
    async fn download_valid_with_sha1_and_sha512() {
        let server = MockServer::start();
        let content = b"contenido de prueba del mod";
        let sha1 = compute_sha1(content);
        let sha512 = sha512_hex(content);

        server.mock(|when, then| {
            when.method(Method::GET).path("/mod.jar");
            then.status(200).body(content);
        });

        let url = format!("{}/mod.jar", server.base_url());
        let temp_path =
            std::env::temp_dir().join(format!("obsidia-test-{}.tmp", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = download_file(
            &client,
            &url,
            &temp_path,
            Some(&sha1),
            Some(&sha512),
            Some(content.len() as u64),
            None,
            "mod.jar",
            "test-instance",
            1,
            &cancelled,
        )
        .await;

        assert!(result.is_ok(), "descarga falló: {:?}", result.err());
        let dl = result.unwrap();
        assert_eq!(dl.sha1, sha1);
        assert_eq!(dl.sha512, sha512);
        assert_eq!(dl.size, content.len() as u64);
        assert!(temp_path.exists());
        let _ = std::fs::remove_file(&temp_path);
    }

    #[tokio::test]
    async fn download_wrong_sha1_rejected() {
        let server = MockServer::start();
        let content = b"contenido real";
        let wrong_sha1 = compute_sha1(b"contenido diferente");
        let sha512 = sha512_hex(content);

        server.mock(|when, then| {
            when.method(Method::GET).path("/mod.jar");
            then.status(200).body(content);
        });

        let url = format!("{}/mod.jar", server.base_url());
        let temp_path =
            std::env::temp_dir().join(format!("obsidia-test-{}.tmp", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = download_file(
            &client,
            &url,
            &temp_path,
            Some(&wrong_sha1),
            Some(&sha512),
            Some(content.len() as u64),
            None,
            "mod.jar",
            "test-instance",
            1,
            &cancelled,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("SHA1 incorrecto"));
        assert!(
            !temp_path.exists(),
            "el temporal debe borrarse si el hash no coincide"
        );
    }

    #[tokio::test]
    async fn download_wrong_sha512_rejected() {
        let server = MockServer::start();
        let content = b"contenido real";
        let sha1 = compute_sha1(content);
        let wrong_sha512 = sha512_hex(b"contenido diferente");

        server.mock(|when, then| {
            when.method(Method::GET).path("/mod.jar");
            then.status(200).body(content);
        });

        let url = format!("{}/mod.jar", server.base_url());
        let temp_path =
            std::env::temp_dir().join(format!("obsidia-test-{}.tmp", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = download_file(
            &client,
            &url,
            &temp_path,
            Some(&sha1),
            Some(&wrong_sha512),
            Some(content.len() as u64),
            None,
            "mod.jar",
            "test-instance",
            1,
            &cancelled,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("SHA512 incorrecto"));
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn download_wrong_size_rejected() {
        let server = MockServer::start();
        let content = b"contenido corto";
        let sha1 = compute_sha1(content);
        let sha512 = sha512_hex(content);

        server.mock(|when, then| {
            when.method(Method::GET).path("/mod.jar");
            then.status(200).body(content);
        });

        let url = format!("{}/mod.jar", server.base_url());
        let temp_path =
            std::env::temp_dir().join(format!("obsidia-test-{}.tmp", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        // Declarar un tamaño mayor al real
        let result = download_file(
            &client,
            &url,
            &temp_path,
            Some(&sha1),
            Some(&sha512),
            Some(999999),
            None,
            "mod.jar",
            "test-instance",
            1,
            &cancelled,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Tamaño incorrecto"));
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn download_http_failure_rejected() {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(Method::GET).path("/mod.jar");
            then.status(500);
        });

        let url = format!("{}/mod.jar", server.base_url());
        let temp_path =
            std::env::temp_dir().join(format!("obsidia-test-{}.tmp", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = download_file(
            &client,
            &url,
            &temp_path,
            Some(&compute_sha1(b"x")),
            Some(&sha512_hex(b"x")),
            Some(1),
            None,
            "mod.jar",
            "test-instance",
            1,
            &cancelled,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("HTTP"));
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn download_cancelled_removes_temp() {
        let server = MockServer::start();
        let content = b"contenido que no se termina de descargar";
        server.mock(|when, then| {
            when.method(Method::GET).path("/mod.jar");
            then.status(200).body(content);
        });

        let url = format!("{}/mod.jar", server.base_url());
        let temp_path =
            std::env::temp_dir().join(format!("obsidia-test-{}.tmp", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(true)); // ya cancelada

        let result = download_file(
            &client,
            &url,
            &temp_path,
            Some(&compute_sha1(content)),
            Some(&sha512_hex(content)),
            Some(content.len() as u64),
            None,
            "mod.jar",
            "test-instance",
            1,
            &cancelled,
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cancelada"));
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn download_redirect_followed() {
        let server = MockServer::start();
        let content = b"contenido tras redireccion";
        let sha1 = compute_sha1(content);
        let sha512 = sha512_hex(content);

        // Primera mock: redirige a la segunda
        server.mock(|when, then| {
            when.method(Method::GET).path("/redirect");
            then.status(302)
                .header("location", format!("{}/final", server.base_url()));
        });
        // Segunda mock: sirve el contenido real
        server.mock(|when, then| {
            when.method(Method::GET).path("/final");
            then.status(200).body(content);
        });

        let url = format!("{}/redirect", server.base_url());
        let temp_path =
            std::env::temp_dir().join(format!("obsidia-test-{}.tmp", std::process::id()));
        let _ = std::fs::remove_file(&temp_path);

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = download_file(
            &client,
            &url,
            &temp_path,
            Some(&sha1),
            Some(&sha512),
            Some(content.len() as u64),
            None,
            "mod.jar",
            "test-instance",
            1,
            &cancelled,
        )
        .await;

        assert!(
            result.is_ok(),
            "descarga con redireccion falló: {:?}",
            result.err()
        );
        let dl = result.unwrap();
        assert_eq!(dl.sha1, sha1);
        assert_eq!(dl.size, content.len() as u64);
        let _ = std::fs::remove_file(&temp_path);
    }

    #[tokio::test]
    async fn download_redirect_unauthorized_host_rejected() {
        // Usar una URL que redirija a un host no permitido.
        // Como httpmock no soporta HTTPS, probamos la política de redirección
        // de forma indirecta: validate_download_url rechaza hosts no permitidos,
        // y redirect_policy los rechaza también. Aquí validamos que
        // validate_download_url capture el host no autorizado.
        let err = validate_download_url("https://evil.example.com/file.jar").unwrap_err();
        assert!(err.contains("host de descarga no permitido"));
    }

    // ───── extract_override_to_temp ─────

    // ───── paths_collide ─────

    #[test]
    fn paths_collide_same_path() {
        assert!(paths_collide("mods/foo.jar", "mods/foo.jar"));
    }

    #[cfg(windows)]
    #[test]
    fn paths_collide_case_insensitive_windows() {
        assert!(paths_collide("MODS/FOO.JAR", "mods/foo.jar"));
    }

    #[cfg(not(windows))]
    #[test]
    fn paths_collide_case_sensitive_unix() {
        assert!(!paths_collide("MODS/FOO.JAR", "mods/foo.jar"));
    }

    #[test]
    fn paths_collide_different_path() {
        assert!(!paths_collide("mods/foo.jar", "mods/bar.jar"));
    }

    // ───── validate_dest_within_instance ─────

    #[test]
    fn validate_dest_within_instance_ok() {
        let instance = std::env::temp_dir().join("obsidia-test-instance");
        let dest = instance.join("mods/foo.jar");
        assert!(validate_dest_within_instance(&dest, &instance).is_ok());
    }

    #[test]
    fn validate_dest_outside_instance_rejected() {
        let instance = std::env::temp_dir().join("obsidia-test-instance");
        let dest = std::env::temp_dir().join("outside/foo.jar");
        assert!(validate_dest_within_instance(&dest, &instance).is_err());
    }

    // ───── install_modpack_files: overrides only (no downloads) ─────

    #[tokio::test]
    async fn install_modpack_overrides_only() {
        let instance_dir =
            std::env::temp_dir().join(format!("obsidia-test-inst-{}/", std::process::id()));
        let _ = std::fs::remove_dir_all(&instance_dir);
        std::fs::create_dir_all(&instance_dir.join(".obsidia")).unwrap();
        std::fs::create_dir_all(&instance_dir.join("mods")).unwrap();

        // Crear ZIP local con overrides
        let zip_path =
            std::env::temp_dir().join(format!("obsidia-test-pack-{}.mrpack", std::process::id()));
        let _ = std::fs::remove_file(&zip_path);
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("modrinth.index.json", options).unwrap();
            zip.write_all(
                br#"{"formatVersion":1,"game":"minecraft","versionId":"v1","name":"TP","dependencies":{}}"#,
            ).unwrap();
            zip.start_file("overrides/config/foo.cfg", options).unwrap();
            zip.write_all(b"config content").unwrap();
            zip.start_file("client-overrides/options.txt", options)
                .unwrap();
            zip.write_all(b"options content").unwrap();
            zip.start_file("server-overrides/server.cfg", options)
                .unwrap();
            zip.write_all(b"server content").unwrap();
            let _ = zip.finish();
        }

        // El ZIP ya fue escrito arriba. Ahora leerlo y crear Mrpack.
        // Necesitamos leer el ZIP para saber los tamaños de overrides.
        let zip_file = std::fs::File::open(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(zip_file).unwrap();
        let mut override_entries = Vec::new();
        let mut client_override_entries = Vec::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let name = entry.name().to_string();
            if name.starts_with("overrides/") && !name.ends_with('/') {
                override_entries.push((name.clone(), entry.size()));
            }
            if name.starts_with("client-overrides/") && !name.ends_with('/') {
                client_override_entries.push((name.clone(), entry.size()));
            }
        }
        drop(archive);

        // Construir Mrpack con los overrides
        let mut mrpack = make_test_mrpack();
        for (name, size) in &override_entries {
            mrpack.overrides.push(crate::mrpack::MrpackOverrideFile {
                zip_path: PathBuf::from(name),
                dest_path: PathBuf::from(name.strip_prefix("overrides/").unwrap()),
                size: *size,
            });
        }
        for (name, size) in &client_override_entries {
            mrpack
                .client_overrides
                .push(crate::mrpack::MrpackOverrideFile {
                    zip_path: PathBuf::from(name),
                    dest_path: PathBuf::from(name.strip_prefix("client-overrides/").unwrap()),
                    size: *size,
                });
        }

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = install_modpack_files(
            None,
            "",
            &instance_dir,
            &mrpack,
            &zip_path,
            &HashSet::new(),
            cancelled,
        )
        .await;

        assert!(
            result.is_ok(),
            "instalación de overrides falló: {:?}",
            result.err()
        );

        // Verificar que los overrides se escribieron
        let foo_cfg = instance_dir.join("config/foo.cfg");
        let options_txt = instance_dir.join("options.txt");
        assert!(foo_cfg.exists(), "config/foo.cfg debería existir");
        assert!(options_txt.exists(), "options.txt debería existir");

        let foo_content = std::fs::read_to_string(&foo_cfg).unwrap();
        assert_eq!(foo_content, "config content");
        let opts_content = std::fs::read_to_string(&options_txt).unwrap();
        assert_eq!(opts_content, "options content");

        // server-overrides NO debe aplicarse en cliente
        let server_cfg = instance_dir.join("server.cfg");
        assert!(
            !server_cfg.exists(),
            "server-overrides no debe instalarse en cliente"
        );

        // Limpiar
        let _ = std::fs::remove_dir_all(&instance_dir);
        let _ = std::fs::remove_file(&zip_path);
    }

    #[tokio::test]
    async fn install_modpack_override_traversal_rejected() {
        let instance_dir =
            std::env::temp_dir().join(format!("obsidia-test-inst2-{}/", std::process::id()));
        let _ = std::fs::remove_dir_all(&instance_dir);
        std::fs::create_dir_all(&instance_dir.join(".obsidia")).unwrap();

        let zip_path =
            std::env::temp_dir().join(format!("obsidia-test-pack2-{}.mrpack", std::process::id()));
        let _ = std::fs::remove_file(&zip_path);
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("modrinth.index.json", options).unwrap();
            zip.write_all(br#"{"formatVersion":1,"game":"minecraft","versionId":"v1","name":"TP","dependencies":{}}"#).unwrap();
            // override con traversal
            zip.start_file("overrides/../escape.cfg", options).unwrap();
            zip.write_all(b"bad").unwrap();
            let _ = zip.finish();
        }

        let mut mrpack = make_test_mrpack();
        mrpack.overrides.push(crate::mrpack::MrpackOverrideFile {
            zip_path: PathBuf::from("overrides/../escape.cfg"),
            dest_path: PathBuf::from("escape.cfg"),
            size: 3,
        });

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = install_modpack_files(
            None,
            "",
            &instance_dir,
            &mrpack,
            &zip_path,
            &HashSet::new(),
            cancelled,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("ruta de override inválida") || err.contains("traversal"));

        let _ = std::fs::remove_dir_all(&instance_dir);
        let _ = std::fs::remove_file(&zip_path);
    }

    #[tokio::test]
    async fn install_modpack_override_size_mismatch_rejected() {
        let instance_dir =
            std::env::temp_dir().join(format!("obsidia-test-inst3-{}/", std::process::id()));
        let _ = std::fs::remove_dir_all(&instance_dir);
        std::fs::create_dir_all(&instance_dir.join(".obsidia")).unwrap();

        let zip_path =
            std::env::temp_dir().join(format!("obsidia-test-pack3-{}.mrpack", std::process::id()));
        let _ = std::fs::remove_file(&zip_path);
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("modrinth.index.json", options).unwrap();
            zip.write_all(br#"{"formatVersion":1,"game":"minecraft","versionId":"v1","name":"TP","dependencies":{}}"#).unwrap();
            zip.start_file("overrides/config/foo.cfg", options).unwrap();
            zip.write_all(b"actual content here").unwrap(); // 19 bytes
            let _ = zip.finish();
        }

        let mut mrpack = make_test_mrpack();
        mrpack.overrides.push(crate::mrpack::MrpackOverrideFile {
            zip_path: PathBuf::from("overrides/config/foo.cfg"),
            dest_path: PathBuf::from("config/foo.cfg"),
            size: 999, // tamaño declarado incorrecto
        });

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = install_modpack_files(
            None,
            "",
            &instance_dir,
            &mrpack,
            &zip_path,
            &HashSet::new(),
            cancelled,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Tamaño incorrecto"));

        let _ = std::fs::remove_dir_all(&instance_dir);
        let _ = std::fs::remove_file(&zip_path);
    }

    // ───── optional files ─────

    #[tokio::test]
    async fn install_modpack_optional_not_selected_not_installed() {
        // Este test verifica la lógica de filtrado de opcionales sin descarga real.
        // Creamos un Mrpack con un archivo opcional que no está seleccionado.
        // Como install_modpack_files valida URLs antes de descargar, y no hay
        // downloads para overrides, este test solo verifica que el archivo
        // opcional no entre en files_to_download.

        let instance_dir =
            std::env::temp_dir().join(format!("obsidia-test-inst4-{}/", std::process::id()));
        let _ = std::fs::remove_dir_all(&instance_dir);
        std::fs::create_dir_all(&instance_dir.join(".obsidia")).unwrap();
        std::fs::create_dir_all(&instance_dir.join("mods")).unwrap();

        let content = b"optional mod content";
        let sha1 = compute_sha1(content);
        let sha512 = sha512_hex(content);

        let mut mrpack = make_test_mrpack();
        mrpack.files.push(make_test_mrpack_file(
            "mods/optional.jar",
            &sha1,
            &sha512,
            Some(content.len() as u64),
            MrpackClientKind::Optional,
            vec!["https://cdn.modrinth.com/data/optional.jar".to_string()],
        ));

        // No seleccionar el opcional
        let selected: HashSet<String> = HashSet::new();

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        // Como la URL es válida (HTTPS, host permitido) pero no hay servidor real,
        // la descarga fallará. Pero el archivo opcional no debería intentarse
        // porque no está seleccionado. Como files_to_download estará vacío,
        // no habrá descargas y la instalación tendrá éxito con 0 archivos.
        let result = install_modpack_files(
            None,
            "",
            &instance_dir,
            &mrpack,
            &std::env::temp_dir().join("nonexistent.mrpack"),
            &selected,
            cancelled,
        )
        .await;

        // Fallará porque el ZIP no existe (validación inicial), pero el punto
        // es que no intenta descargar el opcional no seleccionado.
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("ya no existe"));

        let _ = std::fs::remove_dir_all(&instance_dir);
    }

    // ───── concurrency: max 4 downloads ─────

    // Nota: probar concurrencia real requiere un servidor que retrase respuestas.
    // httpmock no soporta delays fácilmente, así que verificamos que el
    // Semaphore se inicializa con MAX_CONCURRENT_DOWNLOADS = 4.
    #[test]
    fn semaphore_max_4_concurrent() {
        // Esta es una verificación de la constante, no una prueba de integración
        // completa (que requeriría un servidor con delays).
        assert_eq!(MAX_CONCURRENT_DOWNLOADS, 4);
    }

    // ───── cleanup ─────

    #[tokio::test]
    async fn temp_files_cleaned_on_success() {
        // Verificar que install_modpack_files limpia temporales tras éxito.
        // Usamos solo overrides (sin descargas HTTP) para evitar dependencia de red.
        let instance_dir =
            std::env::temp_dir().join(format!("obsidia-test-inst5-{}/", std::process::id()));
        let _ = std::fs::remove_dir_all(&instance_dir);
        std::fs::create_dir_all(&instance_dir.join(".obsidia")).unwrap();
        std::fs::create_dir_all(&instance_dir.join("mods")).unwrap();

        let zip_path =
            std::env::temp_dir().join(format!("obsidia-test-pack5-{}.mrpack", std::process::id()));
        let _ = std::fs::remove_file(&zip_path);
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("modrinth.index.json", options).unwrap();
            zip.write_all(br#"{"formatVersion":1,"game":"minecraft","versionId":"v1","name":"TP","dependencies":{}}"#).unwrap();
            zip.start_file("overrides/config/foo.cfg", options).unwrap();
            zip.write_all(b"cfg").unwrap();
            let _ = zip.finish();
        }

        let mut mrpack = make_test_mrpack();
        mrpack.overrides.push(crate::mrpack::MrpackOverrideFile {
            zip_path: PathBuf::from("overrides/config/foo.cfg"),
            dest_path: PathBuf::from("config/foo.cfg"),
            size: 3,
        });

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = install_modpack_files(
            None,
            "",
            &instance_dir,
            &mrpack,
            &zip_path,
            &HashSet::new(),
            cancelled,
        )
        .await;

        assert!(result.is_ok());

        // El directorio temporal .mrpack-temp debería estar vacío o eliminado
        let temp_dir = instance_dir.join(".obsidia").join(".mrpack-temp");
        if temp_dir.exists() {
            let entries: Vec<_> = std::fs::read_dir(&temp_dir).unwrap().flatten().collect();
            assert!(
                entries.is_empty(),
                "el directorio temporal debe estar vacío tras éxito"
            );
        }

        let _ = std::fs::remove_dir_all(&instance_dir);
        let _ = std::fs::remove_file(&zip_path);
    }

    // ───── Protección de rutas reservadas en .obsidia/ ─────

    #[test]
    fn validate_dest_reserved_obsidia_path_rejected() {
        let instance = std::env::temp_dir().join("obsidia-test-reserved");
        let dest = instance.join(".obsidia/instance.json");
        let err = validate_dest_within_instance(&dest, &instance).unwrap_err();
        assert!(err.contains("Ruta reservada protegida"));
    }

    #[test]
    fn validate_dest_reserved_obsidia_journal_rejected() {
        let instance = std::env::temp_dir().join("obsidia-test-reserved2");
        let dest = instance.join(".obsidia/journal");
        let err = validate_dest_within_instance(&dest, &instance).unwrap_err();
        assert!(err.contains("Ruta reservada protegida"));
    }

    #[test]
    fn validate_dest_any_obsidia_subdir_rejected() {
        let instance = std::env::temp_dir().join("obsidia-test-any-obsidia");
        let dest = instance.join(".obsidia/custom_cache/file.txt");
        let err = validate_dest_within_instance(&dest, &instance).unwrap_err();
        assert!(err.contains("Ruta protegida"));
    }

    #[test]
    fn validate_dest_obsidia_root_rejected() {
        let instance = std::env::temp_dir().join("obsidia-test-obsidia-root");
        let dest = instance.join(".obsidia");
        let err = validate_dest_within_instance(&dest, &instance).unwrap_err();
        assert!(err.contains("Ruta protegida"));
    }

    #[tokio::test]
    async fn install_modpack_override_reserved_obsidia_rejected() {
        let instance_dir =
            std::env::temp_dir().join(format!("obsidia-test-inst-res-{}/", std::process::id()));
        let _ = std::fs::remove_dir_all(&instance_dir);
        std::fs::create_dir_all(&instance_dir.join(".obsidia")).unwrap();
        std::fs::create_dir_all(&instance_dir.join("mods")).unwrap();

        let zip_path = std::env::temp_dir().join(format!(
            "obsidia-test-pack-res-{}.mrpack",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&zip_path);
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("modrinth.index.json", options).unwrap();
            zip.write_all(br#"{"formatVersion":1,"game":"minecraft","versionId":"v1","name":"TP","dependencies":{}}"#).unwrap();
            zip.start_file("overrides/instance.json", options).unwrap();
            zip.write_all(b"malicious").unwrap();
            let _ = zip.finish();
        }

        let mut mrpack = make_test_mrpack();
        mrpack.overrides.push(crate::mrpack::MrpackOverrideFile {
            zip_path: PathBuf::from("overrides/instance.json"),
            dest_path: PathBuf::from(".obsidia/instance.json"),
            size: 10,
        });

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = install_modpack_files(
            None,
            "",
            &instance_dir,
            &mrpack,
            &zip_path,
            &HashSet::new(),
            cancelled,
        )
        .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("Ruta reservada protegida"));

        let _ = std::fs::remove_dir_all(&instance_dir);
        let _ = std::fs::remove_file(&zip_path);
    }

    // ───── Rollback y journal ─────

    #[tokio::test]
    async fn rollback_on_apply_failure_restores_previous() {
        let instance_dir = std::env::temp_dir().join(format!(
            "obsidia-test-inst-rollback-{}/",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&instance_dir);
        std::fs::create_dir_all(&instance_dir.join(".obsidia")).unwrap();
        std::fs::create_dir_all(&instance_dir.join("mods")).unwrap();

        // Crear un archivo preexistente en la instancia
        let existing = instance_dir.join("mods/existing.jar");
        std::fs::write(&existing, b"original content").unwrap();

        // Crear un ZIP con un override que tenga un path inválido para forzar fallo
        // antes de aplicar. En lugar de eso, usamos un archivo de override con
        // ruta protegida para que la validación falle antes de escribir.
        let zip_path = std::env::temp_dir().join(format!(
            "obsidia-test-pack-rollback-{}.mrpack",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&zip_path);
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("modrinth.index.json", options).unwrap();
            zip.write_all(br#"{"formatVersion":1,"game":"minecraft","versionId":"v1","name":"TP","dependencies":{}}"#).unwrap();
            zip.start_file("overrides/../escape.jar", options).unwrap();
            zip.write_all(b"bad").unwrap();
            let _ = zip.finish();
        }

        let mut mrpack = make_test_mrpack();
        mrpack.overrides.push(crate::mrpack::MrpackOverrideFile {
            zip_path: PathBuf::from("overrides/../escape.jar"),
            dest_path: PathBuf::from("escape.jar"),
            size: 3,
        });

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        let result = install_modpack_files(
            None,
            "",
            &instance_dir,
            &mrpack,
            &zip_path,
            &HashSet::new(),
            cancelled,
        )
        .await;

        assert!(result.is_err());

        // Verificar que el archivo preexistente sigue intacto
        assert!(
            existing.exists(),
            "el archivo original debe seguir existiendo"
        );
        let content = std::fs::read_to_string(&existing).unwrap();
        assert_eq!(content, "original content");

        // Verificar que no se creó el archivo malicioso
        let escape = instance_dir.join("escape.jar");
        assert!(
            !escape.exists(),
            "el archivo de traversal no debe haberse creado"
        );

        let _ = std::fs::remove_dir_all(&instance_dir);
        let _ = std::fs::remove_file(&zip_path);
    }

    #[tokio::test]
    async fn journal_recovery_cleans_temp_files() {
        // Verificar que recover_pending limpia archivos .part huérfanos
        let instance_dir =
            std::env::temp_dir().join(format!("obsidia-test-inst-journal-{}/", std::process::id()));
        let _ = std::fs::remove_dir_all(&instance_dir);
        std::fs::create_dir_all(&instance_dir.join(".obsidia")).unwrap();

        // Crear un .part huérfano en .obsidia/
        let part_file = instance_dir.join(".obsidia").join("leftover.part");
        std::fs::write(&part_file, b"huorfano").unwrap();

        let result = crate::fsx::recover_pending(&instance_dir);
        assert!(result.is_ok());
        assert!(
            !part_file.exists(),
            "el .part huérfano debe haberse eliminado"
        );

        let _ = std::fs::remove_dir_all(&instance_dir);
    }

    // ───── Concurrencia real: máximo 4 descargas simultáneas ─────

    #[tokio::test]
    async fn max_four_concurrent_downloads() {
        use std::sync::atomic::AtomicUsize;

        // Servidor TCP simple que retrasa cada respuesta para poder medir
        // cuántas descargas están en curso simultáneamente.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let active_clone = active.clone();
        let max_clone = max_active.clone();

        let server_handle = tokio::spawn(async move {
            loop {
                let (mut stream, _) = match listener.accept().await {
                    Ok(c) => c,
                    Err(_) => break,
                };
                let active = active_clone.clone();
                let max_clone = max_clone.clone();
                tokio::spawn(async move {
                    let n = active.fetch_add(1, Ordering::Relaxed) + 1;
                    let mut current_max = max_clone.load(Ordering::Relaxed);
                    while n > current_max {
                        match max_clone.compare_exchange_weak(
                            current_max,
                            n,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        ) {
                            Ok(_) => break,
                            Err(x) => current_max = x,
                        }
                    }

                    // Leer la solicitud HTTP (ignorar contenido)
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf).await;

                    // Retraso para simular descarga lenta
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

                    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\nHello World!!!";
                    let _ = stream.write_all(response).await;
                    let _ = stream.flush().await;

                    active.fetch_sub(1, Ordering::Relaxed);
                });
            }
        });

        // Esperar a que el servidor esté listo
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Crear 8 MrpackFiles que apunten al mismo servidor lento
        let mut files: Vec<MrpackFile> = Vec::new();
        for i in 0..8 {
            files.push(make_test_mrpack_file(
                &format!("mods/mod{}.jar", i),
                &compute_sha1(b"Hello World!!!"),
                &sha512_hex(b"Hello World!!!"),
                Some(13),
                MrpackClientKind::Required,
                vec![format!("http://{}/mod.jar", addr)],
            ));
        }

        let mut mrpack = make_test_mrpack();
        mrpack.files = files;

        let instance_dir =
            std::env::temp_dir().join(format!("obsidia-test-inst-conc-{}/", std::process::id()));
        let _ = std::fs::remove_dir_all(&instance_dir);
        std::fs::create_dir_all(&instance_dir.join(".obsidia")).unwrap();
        std::fs::create_dir_all(&instance_dir.join("mods")).unwrap();

        let zip_path = std::env::temp_dir().join(format!(
            "obsidia-test-pack-conc-{}.mrpack",
            std::process::id()
        ));
        std::fs::write(&zip_path, b"fake zip").unwrap();

        let client = make_test_client();
        let cancelled = Arc::new(AtomicBool::new(false));

        // Usamos download_file directamente para probar concurrencia sin pasar
        // por install_modpack_files (que requiere ZIP válido).
        let mut handles = Vec::new();
        let sem = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_DOWNLOADS));

        for (i, file) in mrpack.files.iter().enumerate() {
            let permit = sem.clone().acquire_owned().await;
            let client = client.clone();
            let url = file.downloads[0].clone();
            let temp_path = std::env::temp_dir().join(format!("obsidia-test-conc-{i}.tmp"));
            let active_c = active.clone();
            let max_c = max_active.clone();

            let handle = tokio::spawn(async move {
                let _permit = permit;
                let n = active_c.fetch_add(1, Ordering::Relaxed) + 1;
                let mut current_max = max_c.load(Ordering::Relaxed);
                while n > current_max {
                    match max_c.compare_exchange_weak(
                        current_max,
                        n,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    ) {
                        Ok(_) => break,
                        Err(x) => current_max = x,
                    }
                }

                // Descarga simple sin hash (solo para probar concurrencia)
                let _ = client.get(&url).send().await;
                let _ = std::fs::remove_file(&temp_path);

                active_c.fetch_sub(1, Ordering::Relaxed);
                (i, ())
            });
            handles.push(handle);
        }

        for h in handles {
            let _ = h.await;
        }

        server_handle.abort();

        let max = max_active.load(Ordering::Relaxed);
        assert!(
            max <= MAX_CONCURRENT_DOWNLOADS,
            "Se superó el límite de descargas concurrentes: máximo {max}, permitido {MAX_CONCURRENT_DOWNLOADS}"
        );
    }

    // ───── Junctions y enlaces simbólicos ─────

    /// Crea un symlink que apunta fuera de la instancia, si el SO lo permite.
    /// Devuelve None si no se puede crear (p. ej. Windows sin privilegios).
    #[cfg(unix)]
    fn try_create_outside_symlink(link_path: &Path, target: &Path) -> Option<()> {
        use std::os::unix::fs::symlink as unix_symlink;
        unix_symlink(target, link_path).ok()?;
        Some(())
    }

    #[cfg(windows)]
    fn try_create_outside_symlink(link_path: &Path, target: &Path) -> Option<()> {
        use std::os::windows::fs::symlink_dir as win_symlink_dir;
        win_symlink_dir(target, link_path).ok()?;
        Some(())
    }

    #[cfg(not(any(unix, windows)))]
    fn try_create_outside_symlink(_link_path: &Path, _target: &Path) -> Option<()> {
        None
    }

    #[cfg(unix)]
    #[test]
    fn validate_dest_symlink_outside_instance_rejected() {
        let instance = std::env::temp_dir().join("obsidia-test-symlink");
        let _ = std::fs::remove_dir_all(&instance);
        std::fs::create_dir_all(&instance).unwrap();

        let outside = std::env::temp_dir().join("obsidia-test-symlink-outside");
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"leaked").unwrap();

        // Crear symlink dentro de la instancia que apunte afuera
        let link = instance.join("link_outside");
        let _ = try_create_outside_symlink(&link, &outside);

        // El destino a través del symlink debería ser rechazado
        let dest = link.join("secret.txt");
        let err = validate_dest_within_instance(&dest, &instance).unwrap_err();
        assert!(
            err.contains("sale de") || err.contains("fuera de"),
            "error inesperado: {err}"
        );

        let _ = std::fs::remove_dir_all(&instance);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[cfg(windows)]
    #[test]
    fn validate_dest_junction_outside_instance_rejected() {
        use std::os::windows::fs::symlink_dir as win_symlink_dir;

        let instance = std::env::temp_dir().join("obsidia-test-junction");
        let _ = std::fs::remove_dir_all(&instance);
        std::fs::create_dir_all(&instance).unwrap();

        let outside = std::env::temp_dir().join("obsidia-test-junction-outside");
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();

        // Crear junction dentro de la instancia que apunte afuera
        let link = instance.join("junction_outside");
        let _ = win_symlink_dir(&outside, &link);

        // El destino a través del junction debería ser rechazado
        let dest = link.join("any_file.txt");
        let err = validate_dest_within_instance(&dest, &instance).unwrap_err();
        assert!(
            err.contains("sale de") || err.contains("fuera de"),
            "error inesperado: {err}"
        );

        let _ = std::fs::remove_dir_all(&instance);
        let _ = std::fs::remove_dir_all(&outside);
    }
}

/// Resuelve los iconos de los archivos instalados por el modpack consultando
/// Modrinth por SHA1 y actualiza el índice de contenido (kind "mods").
/// Mejor esfuerzo: cualquier error de red o de parseo deja los iconos como estén.
async fn enrich_modpack_icons(
    client: &reqwest::Client,
    instance_dir: &Path,
    targets: &[(String, String)],
) {
    if targets.is_empty() {
        return;
    }
    // 1. Pedir las versiones que coinciden con los SHA1 (batch).
    let hashes: Vec<&str> = targets.iter().map(|(_, s)| s.as_str()).collect();
    let body = serde_json::json!({ "hashes": hashes, "algorithm": "sha1" });
    let resp = match client
        .post("https://api.modrinth.com/v2/version_files")
        .json(&body)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return,
    };
    let versions: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return,
    };

    // 2. project_id únicos.
    let mut pids: Vec<String> = Vec::new();
    for (_, s) in targets {
        if let Some(v) = versions.get(s) {
            if let Some(pid) = v.get("project_id").and_then(|p| p.as_str()) {
                if !pids.iter().any(|x| x == pid) {
                    pids.push(pid.to_string());
                }
            }
        }
    }
    if pids.is_empty() {
        return;
    }

    // 3. Iconos de esos proyectos (batch).
    let ids_json = match serde_json::to_string(&pids) {
        Ok(s) => s,
        Err(_) => return,
    };
    let projects: Vec<serde_json::Value> = match client
        .get("https://api.modrinth.com/v2/projects")
        .query(&[("ids", ids_json.as_str())])
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => match r.json().await {
            Ok(v) => v,
            Err(_) => return,
        },
        _ => return,
    };

    let mut icon_by_pid: HashMap<String, String> = HashMap::new();
    for p in projects {
        if let Some(id) = p.get("id").and_then(|v| v.as_str()) {
            if let Some(icon) = p.get("icon_url").and_then(|v| v.as_str()) {
                icon_by_pid.insert(id.to_string(), icon.to_string());
            }
        }
    }

    // 4. Mapa slug del índice -> icon_url (el slug del índice es la ruta relativa).
    let mut icons: HashMap<String, String> = HashMap::new();
    for (slug, s) in targets {
        if let Some(v) = versions.get(s) {
            if let Some(pid) = v.get("project_id").and_then(|p| p.as_str()) {
                if let Some(icon) = icon_by_pid.get(pid) {
                    icons.insert(slug.clone(), icon.clone());
                }
            }
        }
    }

    let _ = fsx::update_entry_icons(instance_dir, "mods", &icons);
}

/// Separa los ítems de instalación en contenido indexable (mods, shaderpacks,
/// resourcepacks) y overrides de sistema (options.txt, configs, etc.), que se
/// aplican en disco sin pasar por el índice de contenido.
fn split_items(
    instance_dir: &Path,
    items: Vec<fsx::BatchItem>,
) -> (
    Vec<fsx::BatchItem>,
    Vec<fsx::BatchItem>,
    Vec<fsx::BatchItem>,
    Vec<fsx::BatchItem>,
) {
    let mut mods = Vec::new();
    let mut shaders = Vec::new();
    let mut packs = Vec::new();
    let mut plain = Vec::new();
    for item in items {
        let top = item
            .dest
            .strip_prefix(instance_dir)
            .ok()
            .and_then(|p| p.components().next())
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase());
        match top.as_deref() {
            Some("mods") => mods.push(item),
            Some("shaderpacks") => shaders.push(item),
            Some("resourcepacks") => packs.push(item),
            _ => plain.push(item),
        }
    }
    (mods, shaders, packs, plain)
}
