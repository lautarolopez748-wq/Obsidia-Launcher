//! Lector seguro de paquetes `.mrpack` (Modrinth modpack).
//!
//! Un `.mrpack` es un ZIP que contiene `modrinth.index.json` en la raíz y,
//! opcionalmente, directorios `overrides/`, `client-overrides/` y
//! `server-overrides/`.
//!
//! Este módulo SOLO lee y valida: no descarga, no escribe e no instala nada.
//! Toda escritura e instalación queda para bloques posteriores.
//!
//! La API pública se expone para los bloques de implementación siguientes;
//! por ahora solo la ejercen las pruebas unitarias.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use zip::ZipArchive;

/// Tamaño máximo del archivo ZIP en disco (500 MB).
const MAX_ZIP_SIZE: u64 = 500 * 1024 * 1024;
/// Tamaño máximo acumulado descomprimido de las entradas del ZIP (2 GB).
const MAX_UNCOMPRESSED: u64 = 2 * 1024 * 1024 * 1024;
/// Cantidad máxima de entradas en el ZIP.
const MAX_ENTRIES: usize = 10_000;
/// `formatVersion` admitido por la especificación actual de Modrinth.
const FORMAT_VERSION: u32 = 1;
/// El único tipo de juego admitido.
const GAME: &str = "minecraft";

// Dominios de descarga permitidos por la especificación de Modrinth.
// Las solicitudes deben ir a HTTPS y a uno de estos hosts (exacto,
// comparación sin distinguir mayúsculas).
const ALLOWED_DOWNLOAD_HOSTS: &[&str] = &[
    "cdn.modrinth.com",
    "github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
];

// Nombres de directorios de override dentro del ZIP.
const OVERRIDES_DIR: &str = "overrides";
const CLIENT_OVERRIDES_DIR: &str = "client-overrides";
const SERVER_OVERRIDES_DIR: &str = "server-overrides";

// ───────────────────────────── Estructuras de datos ─────────────────────────────

/// Paquete `.mrpack` leído y validado. Listo para ser consumido por el
/// bloque de instalación sin necesidad de volver a validar.
#[derive(Debug, Clone)]
pub struct Mrpack {
    pub format_version: u32,
    pub game: String,
    pub version_id: Option<String>,
    pub name: String,
    pub summary: Option<String>,
    pub dependencies: MrpackDependencies,
    pub files: Vec<MrpackFile>,
    pub overrides: Vec<MrpackOverrideFile>,
    pub client_overrides: Vec<MrpackOverrideFile>,
    /// Tamaño acumulado descomprimido de las entradas del ZIP (bytes).
    pub zip_uncompressed_bytes: u64,
}

/// Dependencias de juego/loader declaradas en el índice.
#[derive(Debug, Clone, Default)]
pub struct MrpackDependencies {
    pub minecraft: Option<String>,
    pub fabric_loader: Option<String>,
    pub forge: Option<String>,
    pub neoforge: Option<String>,
    pub quilt_loader: Option<String>,
    pub other: HashMap<String, String>,
}

/// Un archivo listado en `files` del índice (se descarga desde una URL).
#[derive(Debug, Clone)]
pub struct MrpackFile {
    /// Ruta relativa dentro de la instancia (validada, sin traversal).
    pub path: PathBuf,
    /// Tamaño publicado en el manifiesto (bytes), si lo trae.
    pub size: Option<u64>,
    pub sha1: String,
    pub sha512: String,
    pub downloads: Vec<String>,
    pub env: MrpackEnv,
    /// Clasificación para el cliente, derivada de `env.client`.
    pub client_kind: MrpackClientKind,
}

/// Entorno de un archivo (`client` / `server`).
#[derive(Debug, Clone, Default)]
pub struct MrpackEnv {
    pub client: MrpackEnvValue,
    pub server: Option<MrpackEnvValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MrpackEnvValue {
    #[default]
    Required,
    Optional,
    Unsupported,
}

/// Clasificación de instalación para el cliente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MrpackClientKind {
    /// Instalar obligatoriamente.
    Required,
    /// Presentar al usuario para que elija.
    Optional,
    /// No instalar en el cliente.
    Unsupported,
}

/// Un archivo de override (leaído del contenido del ZIP, no descargado).
#[derive(Debug, Clone)]
pub struct MrpackOverrideFile {
    /// Ruta de la entrada dentro del ZIP (ej. `overrides/config/foo.cfg`).
    pub zip_path: PathBuf,
    /// Ruta destino relativa a la raíz de la instancia (ej. `config/foo.cfg`).
    pub dest_path: PathBuf,
    pub size: u64,
}

// ───────────────────────────── API pública ─────────────────────────────

/// Abre un archivo `.mrpack` desde disco, valida límites y lo interpreta.
///
/// Solo se lee el manifiesto (`modrinth.index.json`). El resto de entradas
/// (sobre todo `overrides/`) se clasifican por ruta y tamaño sin descomprimir
/// su contenido en RAM, de modo que el bloque de instalación pueda reabrir el
/// ZIP por esta misma ruta y leer sobrescrituras una a una.
pub fn read_mrpack(path: &Path) -> Result<Mrpack, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|e| format!("No se pudo leer el paquete {}: {e}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} no es un archivo", path.display()));
    }
    if metadata.len() > MAX_ZIP_SIZE {
        return Err(format!(
            "El paquete supera {} bytes ({} MB)",
            MAX_ZIP_SIZE,
            MAX_ZIP_SIZE / 1024 / 1024
        ));
    }
    let file =
        std::fs::File::open(path).map_err(|e| format!("No se pudo abrir el paquete: {e}"))?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("ZIP inválido o corrupto: {e}"))?;
    parse_zip_entries(&mut archive)
}

/// Interpreta y valida el contenido de un `.mrpack` ya leído en memoria.
///
/// Solo se lee el manifiesto. El llamador conserva `bytes` y puede reabrir el
/// ZIP con `ZipArchive::new(Cursor::new(bytes))` para lectura progresiva.
pub fn read_mrpack_from_bytes(bytes: &[u8]) -> Result<Mrpack, String> {
    if bytes.len() > MAX_ZIP_SIZE as usize {
        return Err("El paquete supera el tamaño máximo admitido".to_string());
    }
    let cursor = Cursor::new(bytes);
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("ZIP inválido o corrupto: {e}"))?;
    parse_zip_entries(&mut archive)
}

/// Recorre las entradas de un ZIP ya abierto sin descomprimir su contenido.
///
/// Solo `modrinth.index.json` se lee completo (es pequeño). Las entradas de
/// `overrides/` y `client-overrides/` se clasifican por ruta y tamaño
/// declarado (`entry.size()`), sin materializar sus bytes en RAM.
fn parse_zip_entries<R: Read + Seek>(archive: &mut ZipArchive<R>) -> Result<Mrpack, String> {
    if archive.len() > MAX_ENTRIES {
        return Err(format!(
            "El paquete tiene {} entradas (máximo {})",
            archive.len(),
            MAX_ENTRIES
        ));
    }

    let mut index_json: Option<Vec<u8>> = None;
    let mut overrides: Vec<MrpackOverrideFile> = Vec::new();
    let mut client_overrides: Vec<MrpackOverrideFile> = Vec::new();
    let mut uncompressed_total: u64 = 0;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("No se pudo leer la entrada {i} del ZIP: {e}"))?;

        // Rechazar entradas especiales (simbolo, dispositivo, etc.): solo
        // archivos regulares y directorios son admisibles.
        if !entry.is_file() && !entry.is_dir() {
            return Err(format!(
                "Entrada ZIP especial no admitida: {}",
                entry.name()
            ));
        }

        let name = entry.name().to_string();
        if name.is_empty() {
            return Err("Entrada ZIP con nombre vacío".to_string());
        }

        let entry_size = entry.size();
        uncompressed_total = uncompressed_total.saturating_add(entry_size);
        if uncompressed_total > MAX_UNCOMPRESSED {
            return Err(format!(
                "El contenido descomprimido supera {} bytes",
                MAX_UNCOMPRESSED
            ));
        }

        if name.ends_with('/') {
            // directorio: nada que leer
            continue;
        }

        if name == "modrinth.index.json" {
            let mut content = Vec::new();
            entry
                .read_to_end(&mut content)
                .map_err(|e| format!("No se pudo leer la entrada {name}: {e}"))?;
            index_json = Some(content);
            continue;
        }

        // Clasificar directorios de override. Solo rutas y tamaño; nada de
        // contenido se conserva en memoria.
        if let Some(stripped) = strip_prefix(&name, OVERRIDES_DIR) {
            let dest = validate_relative_path(stripped)
                .map_err(|e| format!("Ruta de override inválida '{name}': {e}"))?;
            overrides.push(MrpackOverrideFile {
                zip_path: PathBuf::from(name),
                dest_path: dest,
                size: entry_size,
            });
            continue;
        }
        if let Some(stripped) = strip_prefix(&name, CLIENT_OVERRIDES_DIR) {
            let dest = validate_relative_path(stripped)
                .map_err(|e| format!("Ruta de client-overrides inválida '{name}': {e}"))?;
            client_overrides.push(MrpackOverrideFile {
                zip_path: PathBuf::from(name),
                dest_path: dest,
                size: entry_size,
            });
            continue;
        }
        if let Some(_stripped) = strip_prefix(&name, SERVER_OVERRIDES_DIR) {
            // server-overrides se ignora en la instalación cliente.
            continue;
        }
    }

    let index_bytes = index_json
        .ok_or_else(|| "El paquete no contiene modrinth.index.json en la raíz".to_string())?;
    let index: serde_json::Value = serde_json::from_slice(&index_bytes)
        .map_err(|e| format!("modrinth.index.json no es JSON válido: {e}"))?;

    validate_format_version(&index)?;
    validate_game(&index)?;

    let version_id = index
        .get("versionId")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let name = index
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let summary = index
        .get("summary")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let dependencies = parse_dependencies(&index);
    let files = parse_files(index.get("files"))?;

    Ok(Mrpack {
        format_version: FORMAT_VERSION,
        game: GAME.to_string(),
        version_id,
        name,
        summary,
        dependencies,
        files,
        overrides,
        client_overrides,
        zip_uncompressed_bytes: uncompressed_total,
    })
}

/// Versión de Minecraft detectada a partir de las dependencias.
pub fn minecraft_version(mrpack: &Mrpack) -> Option<&str> {
    mrpack.dependencies.minecraft.as_deref()
}

/// Loader detectado a partir de las dependencias, si hay uno único.
pub fn detected_loader(mrpack: &Mrpack) -> Option<&str> {
    if mrpack.dependencies.fabric_loader.is_some() {
        return Some("Fabric");
    }
    if mrpack.dependencies.forge.is_some() {
        return Some("Forge");
    }
    if mrpack.dependencies.neoforge.is_some() {
        return Some("NeoForge");
    }
    if mrpack.dependencies.quilt_loader.is_some() {
        return Some("Quilt");
    }
    None
}

// ───────────────────────────── Validaciones del manifiesto ─────────────────────────────

fn validate_format_version(index: &serde_json::Value) -> Result<(), String> {
    let fv = index
        .get("formatVersion")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "modrinth.index.json no trae formatVersion".to_string())?;
    if fv != FORMAT_VERSION as u64 {
        return Err(format!(
            "formatVersion no admitido: {fv} (se requiere {FORMAT_VERSION})"
        ));
    }
    Ok(())
}

fn validate_game(index: &serde_json::Value) -> Result<(), String> {
    let game = index
        .get("game")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "modrinth.index.json no trae game".to_string())?;
    if game != GAME {
        return Err(format!("Juego no admitido: {game} (se requiere {GAME})"));
    }
    Ok(())
}

fn parse_dependencies(index: &serde_json::Value) -> MrpackDependencies {
    let mut deps = MrpackDependencies::default();
    let obj = match index.get("dependencies") {
        Some(o) if o.is_object() => o,
        _ => return deps,
    };
    for (k, v) in obj.as_object().unwrap() {
        let val = v.as_str().unwrap_or("");
        match k.as_str() {
            "minecraft" => {
                deps.minecraft = if val.is_empty() {
                    None
                } else {
                    Some(val.to_string())
                }
            }
            "fabric-loader" => {
                deps.fabric_loader = if val.is_empty() {
                    None
                } else {
                    Some(val.to_string())
                }
            }
            "forge" => {
                deps.forge = if val.is_empty() {
                    None
                } else {
                    Some(val.to_string())
                }
            }
            "neoforge" => {
                deps.neoforge = if val.is_empty() {
                    None
                } else {
                    Some(val.to_string())
                }
            }
            "quilt-loader" => {
                deps.quilt_loader = if val.is_empty() {
                    None
                } else {
                    Some(val.to_string())
                }
            }
            other => {
                deps.other.insert(other.to_string(), val.to_string());
            }
        }
    }
    deps
}

fn parse_files(files: Option<&serde_json::Value>) -> Result<Vec<MrpackFile>, String> {
    let arr = match files {
        Some(a) if a.is_array() => a.as_array().unwrap(),
        Some(_) => return Err("files debe ser un arreglo".to_string()),
        None => return Ok(Vec::new()), // sin archivos declarados es válido
    };
    let mut out = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let label = format!("files[{i}]");
        let path_str = item
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("{label}: falta path"))?;
        let path = validate_relative_path(path_str)
            .map_err(|e| format!("{label}: ruta inválida '{path_str}': {e}"))?;

        let size = item
            .get("fileSize")
            .and_then(|v| v.as_u64())
            .map(|s| s as u64);

        let hashes = item
            .get("hashes")
            .and_then(|h| h.as_object())
            .ok_or_else(|| format!("{label}: falta hashes"))?;
        let sha1 = require_hash(hashes, "sha1", 40, &label)?;
        let sha512 = require_hash(hashes, "sha512", 128, &label)?;

        let downloads = parse_downloads(item.get("downloads"), &label)?;
        let env = parse_env(item.get("env"));
        let client_kind = client_kind_from_env(&env);

        out.push(MrpackFile {
            path,
            size,
            sha1,
            sha512,
            downloads,
            env,
            client_kind,
        });
    }
    Ok(out)
}

fn require_hash(
    hashes: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    expected_len: usize,
    label: &str,
) -> Result<String, String> {
    let raw = hashes
        .get(field)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("{label}: falta hash {field}"))?;
    if raw.len() != expected_len || !raw.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "{label}: hash {field} mal formado (se esperan {expected_len} hex)"
        ));
    }
    Ok(raw.to_lowercase())
}

fn parse_downloads(
    downloads: Option<&serde_json::Value>,
    label: &str,
) -> Result<Vec<String>, String> {
    let arr = match downloads {
        Some(a) if a.is_array() => a.as_array().unwrap(),
        Some(_) => return Err(format!("{label}: downloads debe ser un arreglo")),
        None => return Err(format!("{label}: falta downloads")),
    };
    if arr.is_empty() {
        return Err(format!(
            "{label}: al menos una URL de descarga es requerida"
        ));
    }
    let mut out = Vec::new();
    for (j, url_val) in arr.iter().enumerate() {
        let url = url_val
            .as_str()
            .ok_or_else(|| format!("{label}: downloads[{j}] no es texto"))?;
        validate_download_url(url).map_err(|e| format!("{label}: downloads[{j}] {e}"))?;
        out.push(url.to_string());
    }
    Ok(out)
}

/// Valida que una URL de descarga use HTTPS y un host permitido.
fn validate_download_url(url: &str) -> Result<(), String> {
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

fn parse_env(env: Option<&serde_json::Value>) -> MrpackEnv {
    let mut e = MrpackEnv::default();
    let obj = match env {
        Some(o) if o.is_object() => o.as_object().unwrap(),
        _ => return e, // sin env -> cliente required por defecto
    };
    e.client = parse_env_value(obj.get("client").and_then(|v| v.as_str()));
    e.server = Some(parse_env_value(obj.get("server").and_then(|v| v.as_str())));
    e
}

fn parse_env_value(raw: Option<&str>) -> MrpackEnvValue {
    match raw {
        Some("optional") => MrpackEnvValue::Optional,
        Some("unsupported") => MrpackEnvValue::Unsupported,
        _ => MrpackEnvValue::Required,
    }
}

fn client_kind_from_env(env: &MrpackEnv) -> MrpackClientKind {
    match env.client {
        MrpackEnvValue::Unsupported => MrpackClientKind::Unsupported,
        MrpackEnvValue::Optional => MrpackClientKind::Optional,
        MrpackEnvValue::Required => MrpackClientKind::Required,
    }
}

// ───────────────────────────── Utilidades de ruta y prefijo ─────────────────────────────

/// Elimina el prefijo de directorio `prefix/` del inicio de `name`.
/// Devuelve `None` si `name` no empieza exactamente con `prefix/`.
fn strip_prefix<'a>(name: &'a str, prefix: &str) -> Option<&'a str> {
    let pfx = format!("{prefix}/");
    name.strip_prefix(&pfx)
}

/// Valida que una ruta sea relativa, segura y no escape del directorio base.
/// Rechaza rutas absolutas, `..`, unidades Windows, UNC y componentes vacíos.
fn validate_relative_path(path: &str) -> Result<PathBuf, String> {
    if path.is_empty() {
        return Err("ruta vacía".to_string());
    }
    if path.contains('\0') {
        return Err("ruta contiene carácter nulo".to_string());
    }

    let pb = Path::new(path);
    if pb.is_absolute() {
        return Err("ruta absoluta no permitida".to_string());
    }

    let first = path.chars().next().unwrap();
    if first == '/' || first == '\\' {
        return Err("ruta inicia con separador de directorio".to_string());
    }

    // Unidad Windows: [A-Z]:/ o [A-Z]:\
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err("ruta con unidad de Windows no permitida".to_string());
    }

    // UNC: \\servidor\...
    if path.starts_with("\\\\") {
        return Err("ruta UNC no permitida".to_string());
    }

    let mut components: Vec<&str> = Vec::new();
    for comp in path.split(['/', '\\']) {
        if comp.is_empty() || comp == "." {
            return Err("componente de ruta vacío o '.' no permitido".to_string());
        }
        if comp == ".." {
            return Err("traversal de directorio '..' no permitido".to_string());
        }
        components.push(comp);
    }

    Ok(PathBuf::from_iter(components))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut buf);
        let options = zip::write::SimpleFileOptions::default();
        for (name, data) in entries {
            zip.start_file(name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
        buf.into_inner()
    }

    fn manifest(
        name: &str,
        version_id: &str,
        mc: &str,
        loader: Option<(&str, &str)>,
        files_json: &str,
    ) -> String {
        let deps = match loader {
            Some((lid, lver)) => format!(r#" "minecraft": "{mc}", "{lid}": "{lver}""#),
            None => format!(r#" "minecraft": "{mc}""#),
        };
        format!(
            r#"{{
                "formatVersion": 1,
                "game": "minecraft",
                "versionId": "{version_id}",
                "name": "{name}",
                "summary": "un modpack de prueba",
                "dependencies": {{{deps}}},
                "files": [{files_json}]
            }}"#
        )
    }

    fn file_entry(
        path: &str,
        sha1: &str,
        sha512: &str,
        size: Option<u64>,
        client: Option<&str>,
        server: Option<&str>,
        downloads: Option<&str>,
    ) -> String {
        let dl = downloads.unwrap_or("https://cdn.modrinth.com/data/x.jar");
        let size_json = match size {
            Some(s) => format!(r#", "fileSize": {s}"#),
            None => String::new(),
        };
        let client_json = match client {
            Some(c) => format!(r#" "client": "{c}""#),
            None => String::new(),
        };
        let server_json = match server {
            Some(s) => format!(r#", "server": "{s}""#),
            None => String::new(),
        };
        format!(
            r#"{{
                "path": "{path}",
                "hashes": {{ "sha1": "{sha1}", "sha512": "{sha512}" }}{size_json},
                "env": {{ {client_json} {server_json} }},
                "downloads": ["{dl}"]
            }}"#
        )
    }

    fn valid_sha1() -> String {
        "a".repeat(40)
    }
    fn valid_sha512() -> String {
        "b".repeat(128)
    }

    #[test]
    fn valido_mrpack_completo() {
        let m = manifest(
            "TestPack",
            "vid-1",
            "1.21.4",
            Some(("fabric-loader", "0.16.0")),
            &file_entry(
                "mods/foo.jar",
                &valid_sha1(),
                &valid_sha512(),
                Some(123),
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        let mp = read_mrpack_from_bytes(&zip).unwrap();
        assert_eq!(mp.name, "TestPack");
        assert_eq!(mp.version_id.as_deref(), Some("vid-1"));
        assert_eq!(minecraft_version(&mp), Some("1.21.4"));
        assert_eq!(detected_loader(&mp), Some("Fabric"));
        assert_eq!(mp.files.len(), 1);
        assert_eq!(mp.files[0].client_kind, MrpackClientKind::Required);
        assert_eq!(mp.files[0].sha1, valid_sha1());
        assert_eq!(mp.files[0].sha512, valid_sha512());
        assert_eq!(mp.files[0].size, Some(123));
    }

    #[test]
    fn zip_corrupto_falla() {
        let res = read_mrpack_from_bytes(b"esto no es un zip");
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("ZIP inválido"));
    }

    #[test]
    fn manifiesto_ausente_falla() {
        let zip = build_zip(&[("otro.txt", b"hola")]);
        let res = read_mrpack_from_bytes(&zip);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("modrinth.index.json"));
    }

    #[test]
    fn formato_incompatible_falla() {
        let m = format!(
            r#"{{
                "formatVersion": 2,
                "game": "minecraft",
                "versionId": "v",
                "name": "x",
                "dependencies": {{ "minecraft": "1.21.4" }},
                "files": []
            }}"#
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        let res = read_mrpack_from_bytes(&zip);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("formatVersion"));
    }

    #[test]
    fn juego_incompatible_falla() {
        let m = format!(
            r#"{{
                "formatVersion": 1,
                "game": "minecraft-pe",
                "versionId": "v",
                "name": "x",
                "dependencies": {{}},
                "files": []
            }}"#
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        let res = read_mrpack_from_bytes(&zip);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Juego no admitido"));
    }

    #[test]
    fn ruta_absoluta_rechazada() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "/mods/foo.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        assert!(read_mrpack_from_bytes(&zip).is_err());
    }

    #[test]
    fn traversal_rechazado() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "../escape.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        assert!(read_mrpack_from_bytes(&zip).is_err());
    }

    #[test]
    fn unidad_windows_rechazada() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "C:/mods/foo.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        assert!(read_mrpack_from_bytes(&zip).is_err());
    }

    #[test]
    fn ruta_unc_rechazada() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "\\\\server\\share\\foo.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        assert!(read_mrpack_from_bytes(&zip).is_err());
    }

    #[test]
    fn hash_ausente_falla() {
        let m = format!(
            r#"{{
                "formatVersion": 1,
                "game": "minecraft",
                "versionId": "v",
                "name": "x",
                "dependencies": {{}},
                "files": [{{
                    "path": "mods/foo.jar",
                    "hashes": {{}},
                    "downloads": ["https://cdn.modrinth.com/data/x.jar"]
                }}]
            }}"#
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        assert!(read_mrpack_from_bytes(&zip).is_err());
    }

    #[test]
    fn hash_mal_formado_falla() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/foo.jar",
                "no-hex!!!",
                &valid_sha512(),
                None,
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        assert!(read_mrpack_from_bytes(&zip).is_err());
    }

    #[test]
    fn sha512_mal_formado_falla() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/foo.jar",
                &valid_sha1(),
                "corto",
                None,
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        assert!(read_mrpack_from_bytes(&zip).is_err());
    }

    #[test]
    fn cliente_requerido_por_defecto_sin_env() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/foo.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        let mp = read_mrpack_from_bytes(&zip).unwrap();
        assert_eq!(mp.files[0].client_kind, MrpackClientKind::Required);
        assert_eq!(mp.files[0].env.client, MrpackEnvValue::Required);
    }

    #[test]
    fn cliente_opcional_se_respeta() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/opt.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                Some("optional"),
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        let mp = read_mrpack_from_bytes(&zip).unwrap();
        assert_eq!(mp.files[0].client_kind, MrpackClientKind::Optional);
    }

    #[test]
    fn cliente_no_compatible_no_se_instala() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/srv.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                Some("unsupported"),
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        let mp = read_mrpack_from_bytes(&zip).unwrap();
        assert_eq!(mp.files[0].client_kind, MrpackClientKind::Unsupported);
    }

    #[test]
    fn server_unsupported_no_excluye_cliente() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/clientonly.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                Some("required"),
                Some("unsupported"),
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        let mp = read_mrpack_from_bytes(&zip).unwrap();
        // server: unsupported no afecta al cliente; el archivo sigue required.
        assert_eq!(mp.files[0].client_kind, MrpackClientKind::Required);
        assert_eq!(mp.files[0].env.server, Some(MrpackEnvValue::Unsupported));
    }

    #[test]
    fn descarga_http_rechazada() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/foo.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                None,
                None,
                Some("http://cdn.modrinth.com/data/x.jar"),
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        assert!(read_mrpack_from_bytes(&zip).is_err());
    }

    #[test]
    fn host_no_permitido_rechazado() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/foo.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                None,
                None,
                Some("https://evil.example.com/data/x.jar"),
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        assert!(read_mrpack_from_bytes(&zip).is_err());
    }

    #[test]
    fn downloads_vacío_rechazado() {
        let m = format!(
            r#"{{
                "formatVersion": 1,
                "game": "minecraft",
                "versionId": "v",
                "name": "x",
                "dependencies": {{}},
                "files": [{{
                    "path": "mods/foo.jar",
                    "hashes": {{ "sha1": "{}", "sha512": "{}" }},
                    "env": {{}},
                    "downloads": []
                }}]
            }}"#,
            valid_sha1(),
            valid_sha512()
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        assert!(read_mrpack_from_bytes(&zip).is_err());
    }

    #[test]
    fn overrides_y_client_overrides_se_clasifican() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/foo.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[
            ("modrinth.index.json", m.as_bytes()),
            ("overrides/config/foo.cfg", b"cfg1"),
            ("client-overrides/options.txt", b"opts"),
            ("server-overrides/server.cfg", b"srv"),
        ]);
        let mp = read_mrpack_from_bytes(&zip).unwrap();
        assert_eq!(mp.overrides.len(), 1);
        assert_eq!(mp.overrides[0].dest_path, PathBuf::from("config/foo.cfg"));
        assert_eq!(mp.client_overrides.len(), 1);
        assert_eq!(
            mp.client_overrides[0].dest_path,
            PathBuf::from("options.txt")
        );
        // server-overrides se ignora en cliente.
        assert!(
            mp.overrides.is_empty() || mp.overrides[0].dest_path != PathBuf::from("server.cfg")
        );
    }

    #[test]
    fn limite_entradas_excedido_falla() {
        let names: Vec<String> = (0..(MAX_ENTRIES + 1))
            .map(|i| format!("filler/{i}.txt"))
            .collect();
        let entries: Vec<(&str, &[u8])> =
            names.iter().map(|s| (s.as_str(), b"x" as &[u8])).collect();
        let zip = build_zip(&entries);
        let res = read_mrpack_from_bytes(&zip);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("entradas"));
    }

    #[test]
    fn detecta_loaders() {
        for (lid, expected) in [
            ("fabric-loader", "Fabric"),
            ("forge", "Forge"),
            ("neoforge", "NeoForge"),
            ("quilt-loader", "Quilt"),
        ] {
            let m = manifest(
                "X",
                "v",
                "1.21.1",
                Some((lid, "1.0")),
                &file_entry(
                    "mods/foo.jar",
                    &valid_sha1(),
                    &valid_sha512(),
                    None,
                    None,
                    None,
                    None,
                ),
            );
            let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
            let mp = read_mrpack_from_bytes(&zip).unwrap();
            assert_eq!(detected_loader(&mp), Some(expected), "loader {lid}");
        }
    }

    #[test]
    fn ruta_override_con_traversal_rechazado() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/foo.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[
            ("modrinth.index.json", m.as_bytes()),
            ("overrides/../evil.cfg", b"bad"),
        ]);
        assert!(read_mrpack_from_bytes(&zip).is_err());
    }

    #[test]
    fn archivo_local_sin_manifest_lee_desde_path() {
        std::fs::create_dir_all(std::env::temp_dir().join("obsidia-mrpack-tests")).unwrap();
        let path = std::env::temp_dir()
            .join("obsidia-mrpack-tests")
            .join("valid.mrpack");
        let m = manifest(
            "DiskPack",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/foo.jar",
                &valid_sha1(),
                &valid_sha512(),
                None,
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        std::fs::write(&path, &zip).unwrap();
        let mp = read_mrpack(&path).unwrap();
        assert_eq!(mp.name, "DiskPack");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn archivo_con_tamano_correcto_se_acepta() {
        let m = manifest(
            "X",
            "v",
            "1.21.4",
            None,
            &file_entry(
                "mods/foo.jar",
                &valid_sha1(),
                &valid_sha512(),
                Some(100),
                None,
                None,
                None,
            ),
        );
        let zip = build_zip(&[("modrinth.index.json", m.as_bytes())]);
        let mp = read_mrpack_from_bytes(&zip).unwrap();
        assert_eq!(mp.files[0].size, Some(100));
    }
}
