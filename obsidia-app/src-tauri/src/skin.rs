//! Skin en modo offline.
//!
//! Minecraft no deja cambiar la skin de una cuenta offline (siempre usa Steve/Alex),
//! así que Obsidia genera un resource pack que reemplaza las texturas del jugador y lo
//! activa en `options.txt`. Solo la ve el jugador: el servidor sigue mostrando Steve.
//!
//! En `options.txt` la lista `resourcePacks` está en orden ascendente de prioridad:
//! la última entrada es la que gana. Por eso ObsidiaSkin se agrega al final.

use base64::Engine;
use serde_json::{Value, json};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::options;

/// Nombre de la carpeta dentro de `resourcepacks`.
const PACK_NAME: &str = "ObsidiaSkin";
/// Texturas por defecto del jugador (1.19.4+ usa nueve skins base).
const PLAYER_SKINS: [&str; 9] = [
    "steve", "alex", "ari", "efe", "kai", "makena", "noor", "sunny", "zuri",
];
/// La skin no puede pesar más que esto (bytes ya decodificados).
const MAX_SKIN_BYTES: usize = 1024 * 1024;

// ────────────────────────────────── Validación ──────────────────────────────────

/// Tamaño (ancho, alto) de un PNG, leído del IHDR.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 24 || bytes[..8] != SIGNATURE || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    Some((w, h))
}

/// Decodifica y valida una skin en base64 (puede venir con el prefijo `data:image/png;base64,`).
/// Acepta 64x64 (formato moderno) y 64x32 (formato viejo). Máximo 1 MB.
pub fn decode_skin(input: &str) -> Result<Vec<u8>, String> {
    let raw = input.trim();
    let b64 = raw.split_once(',').map(|(_, b)| b).unwrap_or(raw);
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| "La skin no es un archivo válido (base64)".to_string())?;
    if bytes.len() > MAX_SKIN_BYTES {
        return Err("La skin pesa más de 1 MB".to_string());
    }
    match png_size(&bytes) {
        Some((64, 64)) | Some((64, 32)) => Ok(bytes),
        Some((w, h)) => Err(format!("La skin mide {w}x{h}: tiene que ser 64x64 o 64x32")),
        None => Err("La skin no es un PNG válido".to_string()),
    }
}

/// Acepta "classic"/"slim" (y sus nombres en español). Cualquier otra cosa es un error.
///
/// El modelo SOLO se valida: el resource pack escribe la misma imagen en las carpetas
/// `wide/` y `slim/`, y Minecraft elige la geometría según el UUID/perfil del jugador,
/// no según el pack. Cambiar la geometría requiere otra vía (perfil o argumentos de
/// lanzamiento) y queda pendiente para una tarea aparte.
pub fn normalize_model(model: &str) -> Result<&'static str, String> {
    match model.trim().to_lowercase().as_str() {
        "classic" | "clasico" | "clásico" => Ok("classic"),
        "slim" | "fino" | "thin" => Ok("slim"),
        _ => Err(format!("Modelo de skin desconocido: {model}")),
    }
}

// ─────────────────────────────── pack.mcmeta ───────────────────────────────────

/// Formato de recursos de un paquete: `major.minor`.
///
/// Los formatos < 65 usan la metadata legacy (`pack_format` + `supported_formats`).
/// Desde 65 Minecraft exige `min_format`/`max_format` y rechaza los campos viejos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResourceFormat {
    major: u32,
    minor: u32,
}

impl ResourceFormat {
    const fn legacy(major: u32) -> Self {
        Self { major, minor: 0 }
    }

    const fn modern(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    fn is_legacy(self) -> bool {
        self.major < 65
    }

    /// Mojang acepta un entero cuando el minor es 0 y un par `[major, minor]`
    /// cuando hay minor. Misma semántica que `PackFormat` de Minecraft.
    fn as_json(self) -> Value {
        if self.minor == 0 {
            json!(self.major)
        } else {
            json!([self.major, self.minor])
        }
    }
}

/// Toma el prefijo numérico de un componente ("9-pre1" → 9).
fn numeric_prefix(value: &str) -> Option<u32> {
    let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse().ok()
    }
}

/// Fallback fiable cuando el JAR del cliente todavía no existe.
///
/// `None` (sin versión) usa el formato legacy 4. Las versiones modernas nunca
/// vuelven a 34: 1.21.9+ usa la metadata nueva.
fn resource_format_for(version: Option<&str>) -> ResourceFormat {
    let Some(v) = version else {
        return ResourceFormat::legacy(4);
    };
    let mut parts = v.trim().split('.');
    let Some(major) = parts.next().and_then(numeric_prefix) else {
        return ResourceFormat::legacy(4);
    };
    let minor = parts.next().and_then(numeric_prefix).unwrap_or(0);
    let patch = parts.next().and_then(numeric_prefix).unwrap_or(0);
    match (major, minor) {
        (1, 0..=8) => ResourceFormat::legacy(1),
        (1, 9..=10) => ResourceFormat::legacy(2),
        (1, 11..=12) => ResourceFormat::legacy(3),
        (1, 13..=14) => ResourceFormat::legacy(4),
        (1, 15) => ResourceFormat::legacy(5),
        (1, 16) if patch <= 1 => ResourceFormat::legacy(5),
        (1, 16) => ResourceFormat::legacy(6),
        (1, 17) => ResourceFormat::legacy(7),
        (1, 18) => ResourceFormat::legacy(8),
        (1, 19) if patch <= 2 => ResourceFormat::legacy(9),
        (1, 19) if patch == 3 => ResourceFormat::legacy(12),
        (1, 19) => ResourceFormat::legacy(13),
        (1, 20) if patch <= 1 => ResourceFormat::legacy(15),
        (1, 20) if patch == 2 => ResourceFormat::legacy(18),
        (1, 20) if patch <= 4 => ResourceFormat::legacy(22),
        (1, 20) => ResourceFormat::legacy(32),
        (1, 21) => match patch {
            0..=1 => ResourceFormat::legacy(34),
            2..=3 => ResourceFormat::legacy(42),
            4 => ResourceFormat::legacy(46),
            5 => ResourceFormat::legacy(55),
            6 => ResourceFormat::legacy(63),
            7..=8 => ResourceFormat::legacy(64),
            9..=10 => ResourceFormat::modern(69, 0),
            11 => ResourceFormat::modern(75, 0),
            // Versión 1.21.x posterior a la última conocida: mantener formato moderno.
            _ => ResourceFormat::modern(75, 0),
        },
        (1, m) if m > 21 => ResourceFormat::modern(75, 0),
        (26, 1) => ResourceFormat::modern(84, 0),
        (26, 2) => ResourceFormat::modern(88, 0),
        (26, 3) => ResourceFormat::modern(97, 1),
        (26, m) if m > 3 => ResourceFormat::modern(97, 1),
        (m, _) if m > 26 => ResourceFormat::modern(97, 1),
        _ => ResourceFormat::legacy(4),
    }
}

/// `pack_format` legacy o major moderno. Para el JSON completo usar `pack_meta_json`.
#[allow(dead_code)]
pub fn pack_format_for(version: Option<&str>) -> u32 {
    resource_format_for(version).major
}

/// Identificador seguro de versión: un único componente de ruta.
fn client_version_id(version: &str) -> Option<&str> {
    let value = version.trim();
    let mut components = Path::new(value).components();
    match (components.next(), components.next()) {
        (Some(std::path::Component::Normal(_)), None) => Some(value),
        _ => None,
    }
}

/// Rutas posibles del JAR del cliente. Cubre `.minecraft` como gameDir y también
/// una instancia en `.minecraft/instances/<id>` (sube hasta la raíz).
fn version_jar_candidates(mc: &Path, version: &str) -> Vec<PathBuf> {
    let relative = Path::new("versions")
        .join(version)
        .join(format!("{version}.jar"));
    let mut candidates = Vec::new();
    let mut current = Some(mc);
    for _ in 0..3 {
        let Some(directory) = current else {
            break;
        };
        let candidate = directory.join(&relative);
        if candidate.is_file() {
            candidates.push(candidate);
        }
        current = directory.parent();
    }
    candidates
}

/// Lee `version.json` en la raíz del JAR y extrae `pack_version`.
fn parse_pack_version(json_text: &str) -> Option<ResourceFormat> {
    let root: Value = serde_json::from_str(json_text).ok()?;
    let pack_version = root.get("pack_version")?;

    if let Some(object) = pack_version.as_object() {
        if let Some(major) = object.get("resource_major").and_then(Value::as_u64) {
            let minor = object
                .get("resource_minor")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            return Some(ResourceFormat {
                major: u32::try_from(major).ok()?,
                minor: u32::try_from(minor).ok()?,
            });
        }
        if let Some(resource) = object.get("resource") {
            return parse_format_value(resource);
        }
    }

    // Esquemas viejos: `"pack_version": 15` o `[major, minor]`.
    parse_format_value(pack_version)
}

fn parse_format_value(value: &Value) -> Option<ResourceFormat> {
    if let Some(number) = value.as_u64() {
        return Some(ResourceFormat::legacy(u32::try_from(number).ok()?));
    }
    if let Some(array) = value.as_array() {
        if array.len() != 2 {
            return None;
        }
        return Some(ResourceFormat {
            major: u32::try_from(array[0].as_u64()?).ok()?,
            minor: u32::try_from(array[1].as_u64()?).ok()?,
        });
    }
    if let Some(object) = value.as_object() {
        let major = u32::try_from(object.get("major")?.as_u64()?).ok()?;
        let minor = object.get("minor").and_then(Value::as_u64).unwrap_or(0);
        return Some(ResourceFormat {
            major,
            minor: u32::try_from(minor).ok()?,
        });
    }
    None
}

fn resource_format_from_jar(path: &Path) -> Option<ResourceFormat> {
    let file = fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let mut entry = archive.by_name("version.json").ok()?;
    let mut json_text = String::new();
    entry.read_to_string(&mut json_text).ok()?;
    parse_pack_version(&json_text)
}

fn resource_format_from_client(mc: &Path, version: &str) -> Option<ResourceFormat> {
    version_jar_candidates(mc, version)
        .into_iter()
        .find_map(|path| resource_format_from_jar(&path))
}

/// Prefiere el `pack_version` real del cliente; si el JAR falta o está roto,
/// cae en la tabla por versión.
fn resource_format_for_client(mc: &Path, version: Option<&str>) -> ResourceFormat {
    let fallback = resource_format_for(version);
    let Some(raw) = version else {
        return fallback;
    };
    let Some(id) = client_version_id(raw) else {
        return fallback;
    };
    resource_format_from_client(mc, id).unwrap_or(fallback)
}

fn pack_meta_json_for_format(format: ResourceFormat) -> String {
    let pack = if format.is_legacy() {
        json!({
            "pack_format": format.major,
            "description": "Obsidia offline skin",
            "supported_formats": {
                "min_inclusive": 1,
                "max_inclusive": 64,
            }
        })
    } else {
        json!({
            "description": "Obsidia offline skin",
            "min_format": format.as_json(),
            "max_format": format.as_json(),
        })
    };
    json!({ "pack": pack }).to_string()
}

/// JSON de `pack.mcmeta` usando solo la tabla por versión.
#[allow(dead_code)]
pub fn pack_meta_json(version: Option<&str>) -> String {
    pack_meta_json_for_format(resource_format_for(version))
}

/// JSON real para el pack, leyendo el JAR del cliente cuando está disponible.
fn pack_meta_json_for_client(mc: &Path, version: Option<&str>) -> String {
    pack_meta_json_for_format(resource_format_for_client(mc, version))
}

// ───────────────────────────── Aplicar / quitar ──────────────────────────────

fn write_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Arma el pack en una carpeta temporal y recién ahí reemplaza la anterior. Si algo
/// falla mientras se escribe, la carpeta vigente queda intacta.
///
/// Si la carpeta no existe pero quedó un `ObsidiaSkin.old` de una interrupción, primero
/// se restaura esa carpeta. Si la restauración falla, se aborta y el respaldo queda
/// intacto: nunca se reemplaza sobre un estado roto.
fn write_pack(mc: &Path, png: &[u8], version: Option<&str>) -> Result<(), String> {
    let packs = mc.join("resourcepacks");
    let target = packs.join(PACK_NAME);
    let tmp = packs.join(format!("{PACK_NAME}.tmp"));
    let backup = packs.join(format!("{PACK_NAME}.old"));

    if !target.exists() && backup.exists() {
        fs::rename(&backup, &target).map_err(|e| {
            format!(
                "{}: {e}. Conservé el respaldo en {}",
                target.display(),
                backup.display()
            )
        })?;
    }

    if tmp.exists() {
        if tmp.is_dir() {
            crate::paths::safe_remove_dir_all(&tmp)?;
        } else {
            return Err(format!("{} existe y no es una carpeta", tmp.display()));
        }
    }
    fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;

    write_file(
        &tmp.join("pack.mcmeta"),
        pack_meta_json_for_client(mc, version).as_bytes(),
    )?;
    write_file(&tmp.join("pack.png"), png)?;

    let entity = tmp
        .join("assets")
        .join("minecraft")
        .join("textures")
        .join("entity");
    for name in PLAYER_SKINS {
        write_file(
            &entity
                .join("player")
                .join("wide")
                .join(format!("{name}.png")),
            png,
        )?;
        write_file(
            &entity
                .join("player")
                .join("slim")
                .join(format!("{name}.png")),
            png,
        )?;
    }
    // Versiones viejas (< 1.19.4) solo conocen estas dos.
    write_file(&entity.join("steve.png"), png)?;
    write_file(&entity.join("alex.png"), png)?;

    let had_old = target.exists();
    if had_old && crate::paths::is_reparse_point(&target) {
        return Err(format!(
            "{} no puede ser un junction ni un symlink",
            target.display()
        ));
    }
    if backup.exists() {
        if backup.is_dir() {
            let _ = crate::paths::safe_remove_dir_all(&backup);
        } else {
            let _ = fs::remove_file(&backup);
        }
    }
    if had_old {
        fs::rename(&target, &backup).map_err(|e| {
            let _ = fs::remove_dir_all(&tmp);
            format!("{}: {e}", target.display())
        })?;
    }
    if let Err(e) = fs::rename(&tmp, &target) {
        if had_old {
            let _ = fs::rename(&backup, &target);
        }
        let _ = fs::remove_dir_all(&tmp);
        return Err(format!("{}: {e}", target.display()));
    }
    if had_old {
        let _ = fs::remove_dir_all(&backup);
    }
    Ok(())
}

/// Genera el resource pack y lo activa con la prioridad más alta en `options.txt`.
///
/// Orden: primero se lee y se calcula el nuevo `options.txt`, después se escribe el
/// pack y recién al final se actualiza `options.txt`. Si la lectura falla no se toca
/// nada; si el pack falla, `options.txt` queda como estaba (nunca apunta a un pack a
/// medio escribir).
pub fn apply_skin(mc: &Path, png: &[u8], model: &str, version: Option<&str>) -> Result<(), String> {
    normalize_model(model)?;

    let options_path = mc.join("options.txt");
    let current = options::read_options(&options_path)?;
    let next = options::edit_resource_packs(&current, true)?;

    let packs = mc.join("resourcepacks");
    if crate::paths::is_reparse_point(&packs) {
        return Err(format!(
            "{} no puede ser un junction ni un symlink",
            packs.display()
        ));
    }

    write_pack(mc, png, version)?;

    if next != current {
        options::write_options(&options_path, &next)?;
    }
    Ok(())
}

/// Saca la entrada de `options.txt` y después borra el pack.
///
/// Ese orden evita dejar una entrada que apunte a una carpeta ya borrada: si el pack
/// no se puede borrar, como mucho queda una carpeta sin usar.
pub fn remove_skin(mc: &Path) -> Result<(), String> {
    let options_path = mc.join("options.txt");
    let current = options::read_options(&options_path)?;
    let next = options::edit_resource_packs(&current, false)?;
    if next != current {
        options::write_options(&options_path, &next)?;
    }

    let packs = mc.join("resourcepacks");
    if crate::paths::is_reparse_point(&packs) {
        return Err(format!(
            "{} no puede ser un junction ni un symlink",
            packs.display()
        ));
    }
    for name in [PACK_NAME, "ObsidiaSkin.tmp", "ObsidiaSkin.old"] {
        let path = packs.join(name);
        if path.exists() {
            if crate::paths::is_reparse_point(&path) {
                return Err(format!(
                    "{} no puede ser un junction ni un symlink",
                    path.display()
                ));
            }
            if path.is_dir() {
                crate::paths::safe_remove_dir_all(&path)?;
            } else {
                fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
    }
    Ok(())
}

// ─────────────────────────────────── Tests ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::*;
    use std::path::PathBuf;

    /// PNG sintético: solo firma + IHDR, suficiente para validar dimensiones.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&width.to_be_bytes());
        b.extend_from_slice(&height.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0]);
        b
    }

    fn b64(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn temp_mc(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "obsidia-skin-{}-{}-{tag}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("resourcepacks")).unwrap();
        dir
    }

    /// JAR mínimo con `version.json` en la raíz, como el cliente real.
    fn version_jar(root: &Path, version: &str, major: u32, minor: u32) {
        use std::io::Write;
        let path = root
            .join("versions")
            .join(version)
            .join(format!("{version}.jar"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("version.json", options).unwrap();
        let json = format!(
            r#"{{"id":"{version}","pack_version":{{"resource_major":{major},"resource_minor":{minor},"data_major":1,"data_minor":0}}}}"#
        );
        zip.write_all(json.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    fn pack_json(text: &str) -> serde_json::Value {
        serde_json::from_str::<serde_json::Value>(text).unwrap()["pack"].clone()
    }

    #[test]
    fn valida_png() {
        assert!(decode_skin(&b64(&png(64, 64))).is_ok());
        assert!(decode_skin(&b64(&png(64, 32))).is_ok());
        assert!(decode_skin(&b64(&png(63, 64))).is_err());
        assert!(decode_skin(&b64(&png(128, 128))).is_err());
        assert!(decode_skin("no es base64 !!").is_err());
        assert!(decode_skin(&b64(b"GIF89a")).is_err());
    }

    #[test]
    fn valida_tamano_maximo() {
        let mut big = png(64, 64);
        big.resize(MAX_SKIN_BYTES + 1, 0);
        assert!(decode_skin(&b64(&big)).is_err());
    }

    #[test]
    fn acepta_data_url() {
        let url = format!("data:image/png;base64,{}", b64(&png(64, 64)));
        assert!(decode_skin(&url).is_ok());
    }

    #[test]
    fn modelos() {
        assert_eq!(normalize_model("Clásico").unwrap(), "classic");
        assert_eq!(normalize_model("slim").unwrap(), "slim");
        assert!(normalize_model("otro").is_err());
    }

    #[test]
    fn packs_ausente() {
        let options = "fov:0.5\nrenderDistance:12\n";
        let out = edit_resource_packs(options, true).unwrap();
        assert!(out.contains("resourcePacks:[\"file/ObsidiaSkin\"]"));
        assert!(out.ends_with('\n'));
        assert!(out.starts_with("fov:0.5\nrenderDistance:12\n"));
    }

    #[test]
    fn packs_vacio() {
        let options = "resourcePacks:[]\n";
        let out = edit_resource_packs(options, true).unwrap();
        assert_eq!(out, "resourcePacks:[\"file/ObsidiaSkin\"]\n");
    }

    #[test]
    fn packs_con_otros_al_final() {
        // La última entrada de options.txt es la de mayor prioridad.
        let options =
            "resourcePacks:[\"file/MiPack.zip\",\"vanilla\"]\nincompatibleResourcePacks:[]\n";
        let out = edit_resource_packs(options, true).unwrap();
        assert!(
            out.contains("resourcePacks:[\"file/MiPack.zip\",\"vanilla\",\"file/ObsidiaSkin\"]")
        );
        assert!(out.contains("incompatibleResourcePacks:[]"));
    }

    #[test]
    fn packs_ya_presente_no_duplica() {
        let options = "resourcePacks:[\"file/ObsidiaSkin\",\"file/Otro\",\"file/ObsidiaSkin\"]\n";
        let out = edit_resource_packs(options, true).unwrap();
        assert_eq!(out.matches("file/ObsidiaSkin").count(), 1);
        assert!(out.contains("resourcePacks:[\"file/Otro\",\"file/ObsidiaSkin\"]"));
    }

    #[test]
    fn packs_quitar() {
        let options =
            "resourcePacks:[\"vanilla\",\"file/ObsidiaSkin\"]\nincompatibleResourcePacks:[]\n";
        let out = edit_resource_packs(options, false).unwrap();
        assert_eq!(
            out,
            "resourcePacks:[\"vanilla\"]\nincompatibleResourcePacks:[]\n"
        );
    }

    #[test]
    fn packs_crlf_y_sin_salto_final() {
        let options = "fov:0.5\r\nresourcePacks:[]";
        let out = edit_resource_packs(options, true).unwrap();
        assert_eq!(out, "fov:0.5\r\nresourcePacks:[\"file/ObsidiaSkin\"]");
        assert!(!out.ends_with('\n'));
    }

    #[test]
    fn packs_clave_duplicada_usa_la_ultima() {
        let options = "resourcePacks:[\"file/viejo.zip\"]\nresourcePacks:[\"vanilla\"]\n";
        let out = edit_resource_packs(options, true).unwrap();
        assert_eq!(
            out,
            "resourcePacks:[\"file/viejo.zip\"]\nresourcePacks:[\"vanilla\",\"file/ObsidiaSkin\"]\n"
        );
    }

    #[test]
    fn json_invalido_es_error() {
        let options = "resourcePacks:[\"file/a.zip\"\n";
        assert!(edit_resource_packs(options, true).is_err());
        assert!(parse_pack_list("resourcePacks:[\"a\",").is_err());
        assert!(parse_pack_list("resourcePacks:").is_err());
    }

    #[test]
    fn options_113_y_121() {
        // 1.13 y 1.21 escriben la misma forma de línea.
        let viejo = "resourcePacks:[]\nincompatibleResourcePacks:[]\n";
        let nuevo = "resourcePacks:[\"vanilla\",\"file/pack.zip\"]\nincompatibleResourcePacks:[\"file/viejo.zip\"]\n";
        let a = edit_resource_packs(viejo, true).unwrap();
        let b = edit_resource_packs(nuevo, true).unwrap();
        assert!(a.contains("resourcePacks:[\"file/ObsidiaSkin\"]"));
        assert!(b.contains("resourcePacks:[\"vanilla\",\"file/pack.zip\",\"file/ObsidiaSkin\"]"));
        assert!(b.contains("incompatibleResourcePacks:[\"file/viejo.zip\"]"));

        assert_eq!(pack_format_for(Some("1.13")), 4);
        assert_eq!(pack_format_for(Some("1.13.2")), 4);
        assert_eq!(pack_format_for(Some("1.21")), 34);
        assert_eq!(pack_format_for(Some("1.21.4")), 46);
        assert!(pack_meta_json(Some("1.13")).contains("\"pack_format\":4"));
        assert!(pack_meta_json(Some("1.21.4")).contains("\"pack_format\":46"));
        assert!(!pack_meta_json(None).contains("999"));
    }

    #[test]
    fn formatos_de_recursos_por_version() {
        let cases = [
            ("1.8.9", 1, 0),
            ("1.12.2", 3, 0),
            ("1.16.5", 6, 0),
            ("1.20.1", 15, 0),
            ("1.21.4", 46, 0),
            ("1.21.5", 55, 0),
            ("1.21.6", 63, 0),
            ("1.21.7", 64, 0),
            ("1.21.8", 64, 0),
            ("1.21.9", 69, 0),
            ("1.21.10", 69, 0),
            ("1.21.11", 75, 0),
            ("26.1", 84, 0),
            ("26.1.2", 84, 0),
            ("26.2", 88, 0),
            ("26.3", 97, 1),
        ];
        for (version, major, minor) in cases {
            let format = resource_format_for(Some(version));
            assert_eq!(format, ResourceFormat { major, minor }, "versión {version}");
            assert_eq!(pack_format_for(Some(version)), major, "versión {version}");
        }
    }

    #[test]
    fn metadatos_legacy_no_superan_64_ni_usan_campos_modernos() {
        let cases = [
            ("1.8.9", 1),
            ("1.12.2", 3),
            ("1.16.5", 6),
            ("1.20.1", 15),
            ("1.21.4", 46),
            ("1.21.5", 55),
            ("1.21.6", 63),
            ("1.21.7", 64),
            ("1.21.8", 64),
        ];
        for (version, format) in cases {
            let text = pack_meta_json(Some(version));
            let pack = pack_json(&text);
            assert_eq!(pack["pack_format"], json!(format), "versión {version}");
            assert_eq!(
                pack["supported_formats"]["min_inclusive"],
                json!(1),
                "versión {version}"
            );
            assert_eq!(
                pack["supported_formats"]["max_inclusive"],
                json!(64),
                "versión {version}"
            );
            assert!(format < 65, "versión {version}");
            assert!(pack.get("min_format").is_none(), "versión {version}");
            assert!(pack.get("max_format").is_none(), "versión {version}");
            assert!(!text.contains("999"), "versión {version}");
        }
    }

    #[test]
    fn metadatos_modernos_no_usan_campos_legacy() {
        let cases = [
            ("1.21.9", json!(69)),
            ("1.21.11", json!(75)),
            ("26.3", json!([97, 1])),
        ];
        for (version, expected) in cases {
            let text = pack_meta_json(Some(version));
            let pack = pack_json(&text);
            assert!(pack.get("pack_format").is_none(), "versión {version}");
            assert!(pack.get("supported_formats").is_none(), "versión {version}");
            assert_eq!(pack["min_format"], expected, "versión {version}");
            assert_eq!(pack["max_format"], expected, "versión {version}");
            assert!(!text.contains("max_inclusive"), "versión {version}");
        }
    }

    #[test]
    fn sin_version_usa_legacy_4_sin_999() {
        let text = pack_meta_json(None);
        let pack = pack_json(&text);
        assert_eq!(pack["pack_format"], json!(4));
        assert_eq!(pack["supported_formats"]["max_inclusive"], json!(64));
        assert!(!text.contains("999"));
    }

    #[test]
    fn interpreta_pack_version_del_cliente() {
        assert_eq!(
            parse_pack_version(
                r#"{"pack_version":{"resource_major":97,"resource_minor":1,"data_major":121,"data_minor":0}}"#
            ),
            Some(ResourceFormat {
                major: 97,
                minor: 1
            })
        );
        assert_eq!(
            parse_pack_version(r#"{"pack_version":{"resource":46,"data":61}}"#),
            Some(ResourceFormat {
                major: 46,
                minor: 0
            })
        );
        assert_eq!(
            parse_pack_version(r#"{"pack_version":15}"#),
            Some(ResourceFormat {
                major: 15,
                minor: 0
            })
        );
        assert_eq!(
            parse_pack_version(r#"{"pack_version":[64,0]}"#),
            Some(ResourceFormat {
                major: 64,
                minor: 0
            })
        );
        assert_eq!(parse_pack_version("{}"), None);
    }

    #[test]
    fn prefiere_el_version_json_del_jar() {
        let root = temp_mc("jar-pref");
        let instance = root.join("instances").join("probe");
        fs::create_dir_all(&instance).unwrap();
        // La tabla diría 46 para 1.21.4; el cliente real manda.
        version_jar(&root, "1.21.4", 97, 1);

        let text = pack_meta_json_for_client(&instance, Some("1.21.4"));
        let pack = pack_json(&text);
        assert_eq!(pack["min_format"], json!([97, 1]));
        assert_eq!(pack["max_format"], json!([97, 1]));
        assert!(pack.get("pack_format").is_none());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fallback_si_el_jar_no_existe_o_esta_roto() {
        let root = temp_mc("jar-fallback");
        let instance = root.join("instances").join("probe");
        fs::create_dir_all(&instance).unwrap();

        let text = pack_meta_json_for_client(&instance, Some("26.3"));
        let pack = pack_json(&text);
        assert_eq!(pack["min_format"], json!([97, 1]));

        let jar = root.join("versions").join("26.3").join("26.3.jar");
        fs::create_dir_all(jar.parent().unwrap()).unwrap();
        fs::write(&jar, b"no es un zip").unwrap();
        let text = pack_meta_json_for_client(&instance, Some("26.3"));
        let pack = pack_json(&text);
        assert_eq!(pack["min_format"], json!([97, 1]));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn apply_skin_26_3_escribe_metadata_moderna() {
        let mc = temp_mc("apply-263");
        fs::write(mc.join("options.txt"), "resourcePacks:[]\n").unwrap();

        apply_skin(&mc, &png(64, 64), "classic", Some("26.3")).unwrap();

        let text = fs::read_to_string(mc.join("resourcepacks").join(PACK_NAME).join("pack.mcmeta"))
            .unwrap();
        let pack = pack_json(&text);
        assert_eq!(pack["min_format"], json!([97, 1]));
        assert_eq!(pack["max_format"], json!([97, 1]));
        assert!(pack.get("pack_format").is_none());
        assert!(pack.get("supported_formats").is_none());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn crea_renglon_sin_salto_final() {
        let out = set_pack_entries("", RESOURCE_PACKS_KEY, &["file/a.zip".to_string()]).unwrap();
        assert_eq!(out, "resourcePacks:[\"file/a.zip\"]");
    }

    #[test]
    fn write_options_conserva_el_respaldo_anterior() {
        let mc = temp_mc("write");
        let path = mc.join("options.txt");
        fs::write(&path, "viejo\n").unwrap();

        write_options(&path, "nuevo\n").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "nuevo\n");
        assert_eq!(
            fs::read_to_string(mc.join("options.txt.bak")).unwrap(),
            "viejo\n"
        );
        assert!(!mc.join("options.txt.tmp").exists());
        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn write_options_no_pierde_el_original_si_falla_el_temporal() {
        let mc = temp_mc("write-tmp");
        let path = mc.join("options.txt");
        fs::write(&path, "original\n").unwrap();
        // Un directorio en el nombre del temporal hace fallar la creación.
        fs::create_dir_all(mc.join("options.txt.tmp")).unwrap();

        assert!(write_options(&path, "nuevo\n").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "original\n");
        assert!(!mc.join("options.txt.bak").exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn write_options_archiva_el_bak_previo_sin_pisarlo() {
        let mc = temp_mc("write-bak");
        let path = mc.join("options.txt");
        fs::write(&path, "original\n").unwrap();
        fs::write(mc.join("options.txt.bak"), "viejo\n").unwrap();

        write_options(&path, "nuevo\n").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "nuevo\n");
        assert_eq!(
            fs::read_to_string(mc.join("options.txt.bak.1")).unwrap(),
            "viejo\n"
        );
        assert_eq!(
            fs::read_to_string(mc.join("options.txt.bak")).unwrap(),
            "original\n"
        );
        assert!(!mc.join("options.txt.tmp").exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn recovery_restaura_options_ausente_con_bak() {
        let mc = temp_mc("recovery-orphan");
        let path = mc.join("options.txt");
        let backup = mc.join("options.txt.bak");
        fs::write(&backup, "fov:0.5\nkey_key.jump:space\n").unwrap();

        assert_eq!(
            read_options(&path).unwrap(),
            "fov:0.5\nkey_key.jump:space\n"
        );
        assert_eq!(
            read_options(&path).unwrap(),
            "fov:0.5\nkey_key.jump:space\n"
        );
        assert!(!backup.exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn recovery_archiva_el_bak_si_options_fue_regenerado() {
        let mc = temp_mc("recovery-regen");
        let path = mc.join("options.txt");
        let backup = mc.join("options.txt.bak");
        // Minecraft regeneró options.txt con valores por defecto.
        fs::write(&path, "fov:0.0\n").unwrap();
        fs::write(&backup, "fov:0.5\nkey_key.jump:space\n").unwrap();

        assert_eq!(read_options(&path).unwrap(), "fov:0.0\n");
        assert_eq!(
            fs::read_to_string(mc.join("options.txt.bak.1")).unwrap(),
            "fov:0.5\nkey_key.jump:space\n"
        );
        assert!(!backup.exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn recovery_no_pisa_respaldos_anteriores() {
        let mc = temp_mc("recovery-unico");
        let path = mc.join("options.txt");
        fs::write(&path, "actual\n").unwrap();
        fs::write(mc.join("options.txt.bak"), "viejo\n").unwrap();
        fs::write(mc.join("options.txt.bak.1"), "mas viejo\n").unwrap();

        read_options(&path).unwrap();

        assert_eq!(
            fs::read_to_string(mc.join("options.txt.bak.1")).unwrap(),
            "mas viejo\n"
        );
        assert_eq!(
            fs::read_to_string(mc.join("options.txt.bak.2")).unwrap(),
            "viejo\n"
        );
        assert!(!mc.join("options.txt.bak").exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn recovery_con_tmp_de_una_interrupcion() {
        let mc = temp_mc("recovery-tmp");
        let path = mc.join("options.txt");
        fs::write(&path, "nuevo\n").unwrap();
        fs::write(mc.join("options.txt.bak"), "viejo\n").unwrap();
        fs::write(mc.join("options.txt.tmp"), "a medias\n").unwrap();

        assert_eq!(read_options(&path).unwrap(), "nuevo\n");
        assert_eq!(
            fs::read_to_string(mc.join("options.txt.bak.1")).unwrap(),
            "viejo\n"
        );
        // El temporal a medias no se toca hasta la próxima escritura.
        assert_eq!(
            fs::read_to_string(mc.join("options.txt.tmp")).unwrap(),
            "a medias\n"
        );

        write_options(&path, "otro\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "otro\n");
        assert_eq!(
            fs::read_to_string(mc.join("options.txt.bak")).unwrap(),
            "nuevo\n"
        );
        assert_eq!(
            fs::read_to_string(mc.join("options.txt.bak.1")).unwrap(),
            "viejo\n"
        );

        let _ = fs::remove_dir_all(&mc);
    }

    #[cfg(windows)]
    fn lock_exclusive(path: &Path) -> fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(path)
            .unwrap()
    }

    #[cfg(windows)]
    #[test]
    fn recovery_fallida_conserva_el_respaldo() {
        let mc = temp_mc("recovery-fail");
        let path = mc.join("options.txt");
        let backup = mc.join("options.txt.bak");
        fs::write(&backup, "viejo\n").unwrap();
        let lock = lock_exclusive(&backup);

        assert!(read_options(&path).is_err());
        assert!(!path.exists());

        drop(lock);
        assert_eq!(fs::read_to_string(&backup).unwrap(), "viejo\n");

        let _ = fs::remove_dir_all(&mc);
    }

    #[cfg(windows)]
    #[test]
    fn write_options_no_pierde_el_original_si_no_puede_respaldar() {
        let mc = temp_mc("write-lock");
        let path = mc.join("options.txt");
        fs::write(&path, "original\n").unwrap();
        let lock = lock_exclusive(&path);

        assert!(write_options(&path, "nuevo\n").is_err());

        drop(lock);
        assert_eq!(fs::read_to_string(&path).unwrap(), "original\n");
        assert!(!mc.join("options.txt.bak").exists());
        assert!(!mc.join("options.txt.tmp").exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn apply_skin_activa_al_final_y_conserva_el_resto() {
        let mc = temp_mc("apply");
        let options = "fov:0.5\nresourcePacks:[\"vanilla\",\"file/Mi.zip\"]\nincompatibleResourcePacks:[\"file/ObsidiaSkin\",\"file/X.zip\"]\n";
        fs::write(mc.join("options.txt"), options).unwrap();

        apply_skin(&mc, &png(64, 64), "classic", Some("1.21.4")).unwrap();

        let out = fs::read_to_string(mc.join("options.txt")).unwrap();
        assert_eq!(
            out,
            "fov:0.5\nresourcePacks:[\"vanilla\",\"file/Mi.zip\",\"file/ObsidiaSkin\"]\nincompatibleResourcePacks:[\"file/X.zip\"]\n"
        );
        let pack = mc.join("resourcepacks").join(PACK_NAME);
        assert!(pack.join("pack.mcmeta").is_file());
        assert!(
            pack.join("assets/minecraft/textures/entity/player/wide/steve.png")
                .is_file()
        );
        assert!(
            pack.join("assets/minecraft/textures/entity/player/slim/alex.png")
                .is_file()
        );
        assert!(!mc.join("resourcepacks").join("ObsidiaSkin.tmp").exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn apply_skin_no_toca_nada_si_no_puede_leer_options() {
        let mc = temp_mc("apply-read");
        // Un directorio donde va el archivo hace fallar la lectura en todas las plataformas.
        fs::create_dir_all(mc.join("options.txt")).unwrap();

        assert!(apply_skin(&mc, &png(64, 64), "slim", None).is_err());
        assert!(mc.join("options.txt").is_dir());
        assert!(!mc.join("resourcepacks").join(PACK_NAME).exists());
        assert!(!mc.join("options.txt.tmp").exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn apply_skin_no_toca_nada_si_el_json_es_invalido() {
        let mc = temp_mc("apply-json");
        let original = "resourcePacks:[\"file/Mi.zip\"\n";
        fs::write(mc.join("options.txt"), original).unwrap();

        assert!(apply_skin(&mc, &png(64, 64), "classic", None).is_err());
        assert_eq!(
            fs::read_to_string(mc.join("options.txt")).unwrap(),
            original
        );
        assert!(!mc.join("resourcepacks").join(PACK_NAME).exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn apply_skin_conserva_crlf_y_sin_salto_final() {
        let mc = temp_mc("apply-crlf");
        fs::write(mc.join("options.txt"), "fov:0.5\r\nresourcePacks:[]").unwrap();

        apply_skin(&mc, &png(64, 64), "classic", None).unwrap();

        let out = fs::read_to_string(mc.join("options.txt")).unwrap();
        assert_eq!(out, "fov:0.5\r\nresourcePacks:[\"file/ObsidiaSkin\"]");

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn remove_skin_quita_entrada_y_pack() {
        let mc = temp_mc("remove");
        fs::write(
            mc.join("options.txt"),
            "resourcePacks:[\"vanilla\",\"file/ObsidiaSkin\",\"file/Mi.zip\"]\n",
        )
        .unwrap();
        fs::create_dir_all(mc.join("resourcepacks").join(PACK_NAME)).unwrap();

        remove_skin(&mc).unwrap();

        assert_eq!(
            fs::read_to_string(mc.join("options.txt")).unwrap(),
            "resourcePacks:[\"vanilla\",\"file/Mi.zip\"]\n"
        );
        assert!(!mc.join("resourcepacks").join(PACK_NAME).exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn remove_skin_no_borra_el_pack_si_no_puede_leer_options() {
        let mc = temp_mc("remove-read");
        let pack = mc.join("resourcepacks").join(PACK_NAME);
        fs::create_dir_all(&pack).unwrap();
        fs::create_dir_all(mc.join("options.txt")).unwrap();

        assert!(remove_skin(&mc).is_err());
        assert!(pack.exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn remove_skin_sin_options_no_crea_el_archivo() {
        let mc = temp_mc("remove-no-options");
        fs::create_dir_all(mc.join("resourcepacks").join(PACK_NAME)).unwrap();

        remove_skin(&mc).unwrap();

        assert!(!mc.join("options.txt").exists());
        assert!(!mc.join("resourcepacks").join(PACK_NAME).exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn apply_skin_recupera_el_pack_tras_una_interrupcion() {
        let mc = temp_mc("apply-recover");
        let packs = mc.join("resourcepacks");
        let old = packs.join("ObsidiaSkin.old");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("marca.txt"), b"viejo").unwrap();
        // Restos del intento anterior: carpeta temporal a medio armar.
        fs::create_dir_all(packs.join("ObsidiaSkin.tmp")).unwrap();
        let options = "resourcePacks:[\"vanilla\",\"file/ObsidiaSkin\"]\n";
        fs::write(mc.join("options.txt"), options).unwrap();

        apply_skin(&mc, &png(64, 64), "classic", Some("1.20.1")).unwrap();

        let target = packs.join(PACK_NAME);
        assert!(target.join("pack.mcmeta").is_file());
        assert!(!target.join("marca.txt").exists());
        assert!(!old.exists());
        assert!(!packs.join("ObsidiaSkin.tmp").exists());
        assert_eq!(fs::read_to_string(mc.join("options.txt")).unwrap(), options);

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn apply_skin_falla_sin_perder_el_pack_recuperado() {
        let mc = temp_mc("apply-recover-fail");
        let packs = mc.join("resourcepacks");
        let old = packs.join("ObsidiaSkin.old");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("marca.txt"), b"viejo").unwrap();
        // Un archivo donde va la carpeta temporal impide armar el pack nuevo.
        fs::write(packs.join("ObsidiaSkin.tmp"), b"x").unwrap();
        let options = "resourcePacks:[\"file/ObsidiaSkin\"]\n";
        fs::write(mc.join("options.txt"), options).unwrap();

        assert!(apply_skin(&mc, &png(64, 64), "slim", None).is_err());

        // El pack anterior se restauró antes de fallar y sigue en su lugar.
        assert!(packs.join(PACK_NAME).join("marca.txt").is_file());
        assert!(!old.exists());
        assert_eq!(fs::read_to_string(mc.join("options.txt")).unwrap(), options);

        let _ = fs::remove_dir_all(&mc);
    }
}
