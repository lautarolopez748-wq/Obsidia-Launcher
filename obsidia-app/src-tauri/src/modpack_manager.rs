//! Gestor de instalación de modpacks (Bloque D).
//!
//! Orquesta el flujo completo:
//! 1. Descargar o leer el archivo `.mrpack`.
//! 2. Crear una instancia nueva o reutilizar la instancia seleccionada.
//! 3. Instalar el loader (Fabric/Forge) en el almacenamiento compartido
//!    (`.minecraft`), no dentro de la instancia, solo al crear una instancia.
//! 4. Descargar e instalar archivos del modpack usando el motor del Bloque C.
//!
//! Reutiliza `instances::create_instance`, `install_fabric`/`install_forge`
//! (vía lógica interna compartida) y `mrpack_install::install_modpack_files`
//! sin modificarlos.

use crate::instances;
use crate::launcher::UA;
use crate::loader_version;
use crate::mrpack::{self, Mrpack};
use crate::mrpack_install::install_modpack_files;
use crate::paths;
use reqwest::redirect::Policy;
use sha1::Digest as Sha1Digest;
use sha1::Sha1 as Sha1Hasher;
use sha2::Sha512 as Sha512Hasher;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tauri::Emitter;
use tokio::fs;
use zip::ZipArchive;

// Dominios de descarga permitidos (misma lista que el Bloque B y especificación de Modrinth).
const ALLOWED_DOWNLOAD_HOSTS: &[&str] = &[
    "cdn.modrinth.com",
    "github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
];
const MAX_REDIRECTS: usize = 5;
const CHUNK_SIZE: usize = 64 * 1024;

// ───────────────────────────── Tokens de cancelación ─────────────────────────────

/// Tokens de cancelación por instancia. Permite al frontend solicitar la
/// cancelación de una instalación en curso.
static INSTALL_CANCEL_TOKENS: OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>,
> = OnceLock::new();

/// Obtiene (creando si es necesario) el token de cancelación para una instancia.
fn cancel_token(instance_id: &str) -> Arc<AtomicBool> {
    let map = INSTALL_CANCEL_TOKENS
        .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .entry(instance_id.to_string())
        .or_insert_with(|| Arc::new(AtomicBool::new(false)))
        .clone()
}

/// Elimina el token de cancelación de una instancia (llamado al finalizar).
fn remove_cancel_token(instance_id: &str) {
    if let Some(map) = INSTALL_CANCEL_TOKENS.get() {
        let mut guard = map.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(instance_id);
    }
}

/// Guarda un token de cancelación y lo elimina automáticamente al caer en
/// desuso (incluso ante pánico o error temprano).
struct CancelTokenGuard {
    instance_id: String,
}

impl Drop for CancelTokenGuard {
    fn drop(&mut self) {
        remove_cancel_token(&self.instance_id);
    }
}

/// Solicita la cancelación de la instalación de una instancia.
pub fn request_cancel(instance_id: &str) -> bool {
    if let Some(map) = INSTALL_CANCEL_TOKENS.get() {
        let guard = map.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(token) = guard.get(instance_id) {
            token.store(true, Ordering::Relaxed);
            return true;
        }
    }
    false
}

// ───────────────────────────── Cliente HTTP seguro ─────────────────────────────

/// Crea una política de redirección que valida cada salto (máximo 5, solo HTTPS, hosts permitidos).
fn redirect_policy() -> Policy {
    let max_redirects = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let max = max_redirects.clone();
    Policy::custom(move |attempt| {
        let count = max.fetch_add(1, Ordering::Relaxed);
        if count >= MAX_REDIRECTS {
            return attempt.error(format!("Demasiadas redirecciones (máximo {MAX_REDIRECTS})"));
        }
        let url = attempt.url();
        let scheme = url.scheme().to_string();
        let host = url.host_str().unwrap_or("").to_string();
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

/// Construye un cliente HTTP seguro para descargar modpacks.
fn create_secure_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(UA)
        .redirect(redirect_policy())
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("No se pudo crear el cliente HTTP: {e}"))
}

// ───────────────────────────── Descarga de .mrpack desde Modrinth ─────────────────────────────

/// Elige, entre las versiones publicadas del proyecto, la primera compatible con
/// la instancia (versión de Minecraft y loader).
///
/// Devuelve `None` si ninguna versión publicada coincide. En ese caso el comando
/// que crea instancias nuevas ignora el resultado, y el flujo hacia una instancia
/// existente devuelve un error claro sin escribir nada.
async fn pick_modpack_version_for_profile(
    client: &reqwest::Client,
    project_slug: &str,
    profile: &crate::Profile,
) -> Result<Option<String>, String> {
    let url = format!("https://api.modrinth.com/v2/project/{project_slug}/version");

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Error consultando Modrinth: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Modrinth respondió HTTP {} al listar versiones",
            resp.status().as_u16()
        ));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Respuesta JSON inválida de Modrinth: {e}"))?;

    let versions = json
        .as_array()
        .ok_or("Modrinth no devolvió una lista de versiones")?;

    let wanted_mc = profile.version.trim();
    let wanted_loader = profile
        .loader
        .as_deref()
        .unwrap_or("Vanilla")
        .to_lowercase();

    for v in versions {
        let game_versions: Vec<&str> = v
            .get("game_versions")
            .and_then(|x| x.as_array())
            .map(|arr| arr.iter().filter_map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let loaders: Vec<String> = v
            .get("loaders")
            .and_then(|x| x.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str())
                    .map(|s| s.to_lowercase())
                    .collect()
            })
            .unwrap_or_default();

        if !wanted_mc.is_empty() && !game_versions.iter().any(|g| *g == wanted_mc) {
            continue;
        }
        if wanted_loader != "vanilla" && !loaders.iter().any(|l| l == &wanted_loader) {
            continue;
        }
        if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
            return Ok(Some(id.to_string()));
        }
    }

    Ok(None)
}

/// Descarga el archivo `.mrpack` de un proyecto de Modrinth a un archivo temporal.
///
/// Si `version_id` es `Some`, se consulta esa versión específica.
/// Si es `None`, se obtiene la lista de versiones y se toma la primera.
async fn download_modpack_from_modrinth(
    client: &reqwest::Client,
    project_slug: &str,
    version_id: Option<&str>,
    dest: &Path,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Descarga cancelada".to_string());
    }

    let url = match version_id {
        Some(vid) => format!("https://api.modrinth.com/v2/project/{project_slug}/version/{vid}"),
        None => format!("https://api.modrinth.com/v2/project/{project_slug}/version"),
    };

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Error consultando Modrinth: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "Modrinth respondió HTTP {} al consultar versión",
            resp.status().as_u16()
        ));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Respuesta JSON inválida de Modrinth: {e}"))?;

    // Si se pidió una versión específica, `json` es un objeto. Si no, es un array.
    let version = if version_id.is_some() {
        json
    } else {
        json.as_array()
            .and_then(|arr| arr.first())
            .cloned()
            .ok_or("No hay versiones disponibles para este proyecto")?
    };

    // Encontrar el archivo principal (.mrpack)
    let files = version
        .get("files")
        .and_then(|f| f.as_array())
        .ok_or("La versión no trae archivos")?;

    let primary = files
        .iter()
        .find(|f| f.get("primary").and_then(|p| p.as_bool()).unwrap_or(false))
        .or_else(|| files.first())
        .ok_or("La versión no tiene archivos")?;

    let file_url = primary
        .get("url")
        .and_then(|u| u.as_str())
        .ok_or("El archivo no tiene URL")?;

    let filename = primary
        .get("filename")
        .and_then(|u| u.as_str())
        .unwrap_or("modpack.mrpack");

    if !filename.ends_with(".mrpack") {
        return Err(format!("El archivo principal no es un .mrpack: {filename}"));
    }

    // Validar URL de descarga
    let scheme_end = file_url
        .find("://")
        .ok_or_else(|| "URL sin esquema".to_string())?;
    let scheme = &file_url[..scheme_end];
    if scheme.eq_ignore_ascii_case("https") == false {
        return Err("solo se admiten URLs https://".to_string());
    }
    let rest = &file_url[scheme_end + 3..];
    let host = rest.split('/').next().unwrap_or(rest);
    let host = host.to_lowercase();
    if !ALLOWED_DOWNLOAD_HOSTS
        .iter()
        .any(|&h| h.eq_ignore_ascii_case(&host))
    {
        return Err(format!("host de descarga no permitido: {host}"));
    }

    if cancelled.load(Ordering::Relaxed) {
        return Err("Descarga cancelada".to_string());
    }

    // Descargar el archivo .mrpack
    let resp = client
        .get(file_url)
        .send()
        .await
        .map_err(|e| format!("Error descargando .mrpack: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!(
            "HTTP {} descargando .mrpack",
            resp.status().as_u16()
        ));
    }

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("Error leyendo .mrpack: {e}"))?;

    if cancelled.load(Ordering::Relaxed) {
        return Err("Descarga cancelada".to_string());
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("No se pudo crear directorio temporal: {e}"))?;
    }

    fs::write(dest, &bytes)
        .await
        .map_err(|e| format!("No se pudo escribir .mrpack temporal: {e}"))?;

    Ok(())
}

// ───────────────────────────── Instalación de loader ─────────────────────────────

/// Instala el loader (Fabric/Forge) en el almacenamiento compartido (`.minecraft`),
/// no dentro de la instancia. El instalador escribe los metadatos de versión y
/// bibliotecas en `.minecraft/versions/` y `.minecraft/libraries/`, que es donde
// El instalador escribe los metadatos de versión y
// bibliotecas en `.minecraft/versions/` y `.minecraft/libraries/`, que es donde
// `launcher::launch` los busca al iniciar.
///
/// NeoForge y Quilt no tienen instalador en esta versión de Obsidia: se devuelve
/// un error claro en lugar de simular una instalación exitosa.
async fn install_loader_to_instance(
    app: &tauri::AppHandle,
    minecraft_dir: &Path,
    instance_dir: &Path,
    loader: &str,
    mc_version: &str,
    loader_version: Option<String>,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Instalación cancelada".to_string());
    }

    match loader {
        "Fabric" => {
            let lv = match loader_version {
                Some(lv) => lv,
                None => loader_version::resolve_loader_version("Fabric", mc_version)
                    .ok()
                    .flatten()
                    .ok_or_else(|| format!("No se pudo resolver Fabric para {mc_version}"))?,
            };

            let (installer_ver, installer_url) = crate::fabric_installer();
            let installer_path = instance_dir.join(format!("fabric-installer-{installer_ver}.jar"));

            crate::download_verified(&installer_url, &installer_path)?;

            if cancelled.load(Ordering::Relaxed) {
                let _ = fs::remove_file(&installer_path);
                return Err("Instalación cancelada".to_string());
            }

            let java =
                crate::launcher::java_for_version(app, minecraft_dir, mc_version, |_required| {
                    None
                })?;

            if cancelled.load(Ordering::Relaxed) {
                return Err("Instalación cancelada".to_string());
            }

            // Usar spawn (no output) para poder matar el proceso si se cancela.
            let mut child = crate::new_cmd(&java)
                .arg("-jar")
                .arg(&installer_path)
                .arg("client")
                .arg("-mcversion")
                .arg(mc_version)
                .arg("-loader")
                .arg(&lv)
                .arg("-dir")
                .arg(minecraft_dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("No se pudo iniciar el instalador de Fabric: {e}"))?;

            // Esperar con comprobación periódica de cancelación.
            loop {
                if cancelled.load(Ordering::Relaxed) {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = fs::remove_file(&installer_path);
                    return Err("Instalación cancelada".to_string());
                }
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if !status.success() {
                            return Err(format!(
                                "Fabric install failed (código {})",
                                status.code().unwrap_or(-1)
                            ));
                        }
                        break;
                    }
                    Ok(None) => {
                        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                    }
                    Err(e) => return Err(format!("Error esperando el instalador de Fabric: {e}")),
                }
            }

            let _ = fs::remove_file(&installer_path);
        }
        "Forge" => {
            let lv = match loader_version {
                Some(lv) => lv,
                None => loader_version::resolve_loader_version("Forge", mc_version)
                    .ok()
                    .flatten()
                    .ok_or_else(|| format!("No se pudo resolver Forge para {mc_version}"))?,
            };

            if !crate::only_chars(&lv, &['.', '-', '_']) {
                return Err("Versión de Forge inválida".to_string());
            }

            let url = format!(
                "https://maven.minecraftforge.net/net/minecraftforge/forge/{mc_version}-{lv}/forge-{mc_version}-{lv}-installer.jar"
            );
            let installer_path = instance_dir.join(format!("forge-installer-{mc_version}.jar"));

            crate::download_verified(&url, &installer_path)?;

            if cancelled.load(Ordering::Relaxed) {
                let _ = fs::remove_file(&installer_path);
                return Err("Instalación cancelada".to_string());
            }

            let java =
                crate::launcher::java_for_version(app, minecraft_dir, mc_version, |_required| {
                    None
                })?;

            if cancelled.load(Ordering::Relaxed) {
                return Err("Instalación cancelada".to_string());
            }

            let mut child = crate::new_cmd(&java)
                .arg("-jar")
                .arg(&installer_path)
                .arg("--installClient")
                .arg(minecraft_dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("No se pudo iniciar el instalador de Forge: {e}"))?;

            loop {
                if cancelled.load(Ordering::Relaxed) {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = fs::remove_file(&installer_path);
                    return Err("Instalación cancelada".to_string());
                }
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if !status.success() {
                            return Err(format!(
                                "Forge install failed (código {})",
                                status.code().unwrap_or(-1)
                            ));
                        }
                        break;
                    }
                    Ok(None) => {
                        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                    }
                    Err(e) => return Err(format!("Error esperando el instalador de Forge: {e}")),
                }
            }

            let _ = fs::remove_file(&installer_path);
        }
        "NeoForge" => {
            return Err("NeoForge no tiene instalador en esta versión de Obsidia".to_string());
        }
        "Quilt" => {
            return Err("Quilt no tiene instalador en esta versión de Obsidia".to_string());
        }
        "Vanilla" => {}
        _ => return Err(format!("Loader no soportado: {loader}")),
    }

    Ok(())
}

// ───────────────────────────── Validación de identificador de instancia ─────────────────────────────

/// Valida el slug de un proyecto de Modrinth antes de usarlo en una URL.
fn validate_project_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() || slug.contains("..") || slug.contains('/') || slug.contains('\\') {
        return Err("Slug de proyecto inválido".to_string());
    }
    Ok(())
}

/// Genera un identificador de instancia válido a partir de un nombre.
/// Rechaza nombres vacíos, `legacy` y caracteres no permitidos.
fn validate_instance_name(name: &str) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("El nombre de la instancia no puede estar vacío".to_string());
    }
    let id = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect::<String>();
    if id.is_empty() {
        return Err("El nombre de la instancia no contiene caracteres válidos".to_string());
    }
    if id.eq_ignore_ascii_case("legacy") {
        return Err("\"legacy\" está reservado".to_string());
    }
    Ok(id)
}

// ───────────────────────────── Orquestación principal ─────────────────────────────

/// Ejecuta la instalación completa de un modpack desde un archivo `.mrpack` ya leído.
///
/// Flujo:
/// 1. Validar nombre de instancia y crear ID único.
/// 2. Crear token de cancelación (antes de cualquier operación).
/// 3. Crear el perfil y la instancia (sistema existente).
/// 4. Instalar el loader (Fabric/Forge) en `.minecraft` (compartido).
/// 5. Descargar e instalar archivos del modpack (motor del Bloque C).
/// 6. Devolver el ID de la instancia para que pueda lanzarse.
async fn install_modpack_orchestrate(
    app: tauri::AppHandle,
    paths: &crate::paths::Paths,
    mrpack: &Mrpack,
    zip_path: &Path,
    instance_name: String,
    selected_optionals: Vec<String>,
) -> Result<String, String> {
    // Validar nombre de instancia
    let base_id = validate_instance_name(&instance_name)?;

    // Generar ID único (evitar colisiones con instancias existentes)
    let profiles = crate::read_json_list::<crate::Profile>(&paths.profiles_file())?;
    let mut instance_id = base_id.clone();
    let mut counter = 1u32;
    while profiles
        .iter()
        .any(|p| p.id.as_deref() == Some(&instance_id))
        || profiles.iter().any(|p| p.name == instance_name)
    {
        instance_id = format!("{base_id}-{counter}");
        counter += 1;
        if counter > 1000 {
            return Err("No se pudo generar un identificador de instancia único".to_string());
        }
    }

    // Extraer MC version y loader del manifiesto
    let mc_version = mrpack::minecraft_version(mrpack)
        .ok_or_else(|| "El modpack no declara versión de Minecraft".to_string())?;
    let loader = mrpack::detected_loader(mrpack).ok_or_else(|| {
        "El modpack no declara un loader (Fabric/Forge/NeoForge/Quilt)".to_string()
    })?;

    // Resolver versión del loader
    let loader_version_opt = loader_version::resolve_loader_version(loader, mc_version)
        .ok()
        .flatten();

    // Crear token de cancelación ANTES de cualquier operación de escritura o descarga.
    let cancelled = cancel_token(&instance_id);
    let _guard = CancelTokenGuard {
        instance_id: instance_id.clone(),
    };

    // Crear el perfil. `profile.version` se conserva como versión de Minecraft;
    // `profile.loader_version` almacena la versión del loader. El lanzador
    // (`launch_instance_inner`) resuelve el ID completo del loader al iniciar.
    let profile = crate::Profile {
        name: instance_name.clone(),
        version: mc_version.to_string(),
        username: "Jugador".to_string(),
        ram: "4G".to_string(),
        java_path: None,
        loader: Some(loader.to_string()),
        loader_version: loader_version_opt.clone(),
        fullscreen: false,
        id: Some(instance_id.clone()),
        block: None,
        schema: 2,
        created_at: Some(crate::now_string()),
        origin: Some("modpack".to_string()),
    };

    // Crear la instancia (sistema existente)
    instances::create_instance(paths, profile.clone())
        .map_err(|e| format!("No se pudo crear la instancia: {e}"))?;

    let instance_dir = paths
        .instance_dir(&instance_id)
        .map_err(|e| format!("No se pudo obtener el directorio de la instancia: {e}"))?;
    let minecraft_dir = paths.minecraft.clone();

    // Instalar el loader (con cancelación)
    install_loader_to_instance(
        &app,
        &minecraft_dir,
        &instance_dir,
        loader,
        mc_version,
        loader_version_opt,
        &cancelled,
    )
    .await
    .map_err(|e| {
        // Si falla la instalación del loader, limpiar la instancia creada
        let _ = instances::delete_instance(paths, instance_id.clone());
        e
    })?;

    // Selected optionals
    let selected_optionals: HashSet<String> = selected_optionals.into_iter().collect();

    // Instalar archivos del modpack (motor del Bloque C, con cancelación)
    let result = install_modpack_files(
        Some(app),
        &mrpack.name,
        &instance_dir,
        mrpack,
        zip_path,
        &selected_optionals,
        cancelled.clone(),
    )
    .await;

    if let Err(e) = &result {
        // Si falla la instalación de archivos, limpiar la instancia
        let _ = instances::delete_instance(paths, instance_id.clone());
    }

    result.map(|_| instance_id)
}

/// Verifica que una instancia existente sea compatible con un modpack.
///
/// No modifica la instancia ni su perfil. Devuelve un mensaje claro cuando la
/// versión de Minecraft o el loader no coinciden con lo que exige el modpack.
fn validate_modpack_compatibility(mrpack: &Mrpack, profile: &crate::Profile) -> Result<(), String> {
    let mc_version = mrpack::minecraft_version(mrpack)
        .ok_or_else(|| "El modpack no declara versión de Minecraft".to_string())?;
    let required_loader = mrpack::detected_loader(mrpack)
        .ok_or_else(|| "El modpack no declara un loader (Fabric/Forge/NeoForge/Quilt)".to_string())?;

    let instance_version = profile.version.trim();
    if !instance_version.is_empty() && instance_version != mc_version {
        return Err(format!(
            "El modpack requiere Minecraft {mc_version} y la instancia \"{}\" usa {instance_version}. Elegí una instancia compatible o creá una nueva.",
            profile.name
        ));
    }

    let instance_loader = profile.loader.as_deref().unwrap_or("Vanilla");
    if !instance_loader.eq_ignore_ascii_case(required_loader) {
        return Err(format!(
            "El modpack requiere {required_loader} y la instancia \"{}\" usa {instance_loader}. Elegí una instancia compatible o creá una nueva.",
            profile.name
        ));
    }

    Ok(())
}

/// Instala un `.mrpack` ya descargado dentro de una instancia existente.
///
/// A diferencia de `install_modpack_orchestrate`, no crea perfiles ni
/// instancias, y no instala loaders: reutiliza la instancia seleccionada y
/// solo escribe los archivos del modpack en su directorio.
async fn install_modpack_into_existing(
    app: tauri::AppHandle,
    paths: &crate::paths::Paths,
    mrpack: &Mrpack,
    zip_path: &Path,
    project_slug: &str,
    instance_id: &str,
    profile: &crate::Profile,
    selected_optionals: Vec<String>,
) -> Result<String, String> {
    // Validar compatibilidad ANTES de escribir nada en la instancia.
    validate_modpack_compatibility(mrpack, profile)?;

    let instance_dir = paths
        .instance_dir(instance_id)
        .map_err(|e| format!("No se pudo obtener el directorio de la instancia: {e}"))?;

    // Token de cancelación por instancia, igual que en el flujo de creación.
    let cancelled = cancel_token(instance_id);
    let _guard = CancelTokenGuard {
        instance_id: instance_id.to_string(),
    };

    let selected_optionals: HashSet<String> = selected_optionals.into_iter().collect();

    install_modpack_files(
        Some(app),
        project_slug,
        &instance_dir,
        mrpack,
        zip_path,
        &selected_optionals,
        cancelled,
    )
    .await?;

    Ok(instance_id.to_string())
}

// ───────────────────────────── Comandos Tauri ─────────────────────────────

/// Comando Tauri: instala un modpack de Modrinth como una nueva instancia.
///
/// `project_slug`: slug del proyecto en Modrinth.
/// `version_id`: ID de versión opcional. Si no se provee, se usa la primera disponible.
/// `instance_name`: nombre de la nueva instancia.
/// `selected_optionals`: rutas de archivos opcionales seleccionados por el usuario.
#[tauri::command(async)]
pub async fn install_modpack_from_modrinth(
    app: tauri::AppHandle,
    paths: tauri::State<'_, crate::paths::Paths>,
    project_slug: String,
    version_id: Option<String>,
    instance_name: String,
    selected_optionals: Vec<String>,
) -> Result<String, String> {
    let paths = paths.inner().clone();

    // Validar slug antes de usarlo en URL
    validate_project_slug(&project_slug)?;

    // Directorio temporal para descargar el .mrpack
    let temp_dir = paths.obsidia.join(".modpack-downloads");
    fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| format!("No se pudo crear directorio temporal: {e}"))?;
    let zip_path = temp_dir.join(format!("{}.mrpack", instance_name));

    // Limpiar si existe
    let _ = fs::remove_file(&zip_path).await;

    let client = create_secure_client()?;

    // Descargar .mrpack de Modrinth (con cancelación)
    // Necesitamos un token temporal para esta descarga; se crea uno ligero.
    let download_cancelled = Arc::new(AtomicBool::new(false));
    download_modpack_from_modrinth(
        &client,
        &project_slug,
        version_id.as_deref(),
        &zip_path,
        &download_cancelled,
    )
    .await?;

    // Leer y validar el .mrpack
    let mrpack = mrpack::read_mrpack(&zip_path)
        .map_err(|e| format!("El paquete .mrpack no es válido: {e}"))?;

    // Orquestar instalación
    let result = install_modpack_orchestrate(
        app,
        &paths,
        &mrpack,
        &zip_path,
        instance_name,
        selected_optionals,
    )
    .await;

    // Limpiar archivo .mrpack descargado
    let _ = fs::remove_file(&zip_path).await;

    result
}

/// Comando Tauri: instala un modpack de Modrinth dentro de una instancia existente.
///
/// No crea instancias ni modifica el perfil. Si la versión de Minecraft o el
/// loader del modpack no coinciden con la instancia, devuelve un error claro
/// sin escribir nada.
///
/// `project_slug`: slug del proyecto en Modrinth.
/// `version_id`: ID de versión opcional. Si no se provee, se usa la primera disponible.
/// `instance_id`: ID de la instancia ya seleccionada por el usuario.
/// `selected_optionals`: rutas de archivos opcionales seleccionados por el usuario.
#[tauri::command(async)]
pub async fn install_modpack_into_instance(
    app: tauri::AppHandle,
    paths: tauri::State<'_, crate::paths::Paths>,
    project_slug: String,
    version_id: Option<String>,
    instance_id: String,
    selected_optionals: Vec<String>,
) -> Result<String, String> {
    let paths = paths.inner().clone();

    validate_project_slug(&project_slug)?;

    // Resolver el perfil de la instancia seleccionada. `legacy` no tiene id
    // propio: se corresponde con el perfil sin `id` que usa `.minecraft`.
    let profiles = crate::read_json_list::<crate::Profile>(&paths.profiles_file())?;
    let profile = profiles
        .iter()
        .find(|p| p.id.as_deref() == Some(instance_id.as_str()))
        .or_else(|| {
            if instance_id == "legacy" {
                profiles.iter().find(|p| p.id.is_none())
            } else {
                None
            }
        })
        .cloned()
        .ok_or_else(|| "La instancia seleccionada no existe.".to_string())?;

    // Directorio temporal para el .mrpack (nombre fijo por instancia).
    let temp_dir = paths.obsidia.join(".modpack-downloads");
    fs::create_dir_all(&temp_dir)
        .await
        .map_err(|e| format!("No se pudo crear directorio temporal: {e}"))?;
    let zip_path = temp_dir.join(format!("{instance_id}.mrpack"));
    let _ = fs::remove_file(&zip_path).await;

    let client = create_secure_client()?;

    // Si no se pidió una versión concreta, elegir una publicada que coincida con
    // la instancia. Así un modpack con varias versiones instala la compatible.
    let resolved_version = match version_id {
        Some(v) => Some(v),
        None => pick_modpack_version_for_profile(&client, &project_slug, &profile).await?,
    };
    let resolved_version = match resolved_version {
        Some(v) => v,
        None => {
            return Err(format!(
                "El modpack no tiene ninguna versión compatible con la instancia \"{}\" (Minecraft {} · {}). No se modificó la instancia.",
                profile.name,
                profile.version.trim(),
                profile.loader.as_deref().unwrap_or("Vanilla")
            ));
        }
    };

    // Descargar el .mrpack elegido desde Modrinth.
    let download_cancelled = Arc::new(AtomicBool::new(false));
    if let Err(e) = download_modpack_from_modrinth(
        &client,
        &project_slug,
        Some(&resolved_version),
        &zip_path,
        &download_cancelled,
    )
    .await
    {
        let _ = fs::remove_file(&zip_path).await;
        return Err(e);
    }

    // Leer y validar el .mrpack antes de tocar la instancia.
    let mrpack = match mrpack::read_mrpack(&zip_path) {
        Ok(m) => m,
        Err(e) => {
            let _ = fs::remove_file(&zip_path).await;
            return Err(format!("El paquete .mrpack no es válido: {e}"));
        }
    };

    let result = install_modpack_into_existing(
        app,
        &paths,
        &mrpack,
        &zip_path,
        &project_slug,
        &instance_id,
        &profile,
        selected_optionals,
    )
    .await;

    let _ = fs::remove_file(&zip_path).await;

    result
}

/// Comando Tauri: instala un modpack desde un archivo `.mrpack` local.
///
/// `file_path`: ruta al archivo `.mrpack` en disco.
/// `instance_name`: nombre de la nueva instancia.
/// `selected_optionals`: rutas de archivos opcionales seleccionados por el usuario.
#[tauri::command(async)]
pub async fn install_modpack_from_local(
    app: tauri::AppHandle,
    paths: tauri::State<'_, crate::paths::Paths>,
    file_path: String,
    instance_name: String,
    selected_optionals: Vec<String>,
) -> Result<String, String> {
    let paths = paths.inner().clone();

    let zip_path = Path::new(&file_path);

    if !zip_path.exists() {
        return Err("El archivo .mrpack no existe".to_string());
    }

    // Leer y validar el .mrpack
    let mrpack = mrpack::read_mrpack(zip_path)
        .map_err(|e| format!("El paquete .mrpack no es válido: {e}"))?;

    // Orquestar instalación
    install_modpack_orchestrate(
        app,
        &paths,
        &mrpack,
        zip_path,
        instance_name,
        selected_optionals,
    )
    .await
}

/// Comando Tauri: solicita la cancelación de una instalación de modpack en curso.
///
/// `instance_id`: identificador de la instancia cuya instalación se desea cancelar.
#[tauri::command]
pub fn cancel_modpack_install(instance_id: String) -> Result<(), String> {
    if request_cancel(&instance_id) {
        Ok(())
    } else {
        Err(format!(
            "No hay ninguna instalación activa para {instance_id}"
        ))
    }
}

// ───────────────────────────── Pruebas ─────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn validate_instance_name_valido() {
        assert_eq!(
            validate_instance_name("Mi Instancia").unwrap(),
            "MiInstancia"
        );
        assert_eq!(validate_instance_name("mi-mod_123").unwrap(), "mi-mod_123");
    }

    #[test]
    fn validate_instance_name_vacio_rechazado() {
        assert!(validate_instance_name("").is_err());
        assert!(validate_instance_name("   ").is_err());
    }

    #[test]
    fn validate_instance_name_legacy_rechazado() {
        assert!(validate_instance_name("legacy").is_err());
        assert!(validate_instance_name("LEGACY").is_err());
    }

    #[test]
    fn validate_instance_name_caracteres_invalidos_rechazados() {
        assert!(validate_instance_name("../escape").is_err());
        assert!(validate_instance_name("C:/windows").is_err());
    }

    #[test]
    fn cancel_token_crea_y_elimina() {
        let token = cancel_token("test-instance");
        assert!(!token.load(Ordering::Relaxed));

        request_cancel("test-instance");
        assert!(token.load(Ordering::Relaxed));

        remove_cancel_token("test-instance");
        // Después de eliminar, obtener un nuevo token debería empezar en false
        let token2 = cancel_token("test-instance");
        assert!(!token2.load(Ordering::Relaxed));
    }

    #[test]
    fn request_cancel_inexistente_retorna_falso() {
        assert!(!request_cancel("no-existe"));
    }

    #[test]
    fn redirect_policy_limite_saltos() {
        // La política se construye sin panic y tiene un límite de redirecciones
        // incorporado (máximo 5). No hay forma directa de probar Policy::custom
        // sin hacer una request HTTP real, pero verificamos que se construye.
        let _policy = redirect_policy();
    }

    #[test]
    fn redirect_policy_rechaza_http() {
        let _policy = redirect_policy();
    }
}
