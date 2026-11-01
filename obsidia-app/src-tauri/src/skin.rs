//! Skin en modo offline.
//!
//! Minecraft no deja cambiar la skin de una cuenta offline (siempre usa Steve/Alex),
//! así que Obsidia genera un resource pack que reemplaza las texturas del jugador y lo
//! activa en `options.txt`. Solo la ve el jugador: el servidor sigue mostrando Steve.

use base64::Engine;
use std::fs;
use std::path::Path;

/// Nombre de la carpeta dentro de `resourcepacks`.
const PACK_NAME: &str = "ObsidiaSkin";
/// Entrada tal como la escribe Minecraft en `options.txt`.
const PACK_ENTRY: &str = "file/ObsidiaSkin";
/// Texturas por defecto del jugador (1.19.4+ usa nueve skins base).
const PLAYER_SKINS: [&str; 9] = ["steve", "alex", "ari", "efe", "kai", "makena", "noor", "sunny", "zuri"];
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
pub fn normalize_model(model: &str) -> Result<&'static str, String> {
    match model.trim().to_lowercase().as_str() {
        "classic" | "clasico" | "clásico" => Ok("classic"),
        "slim" | "fino" | "thin" => Ok("slim"),
        _ => Err(format!("Modelo de skin desconocido: {model}")),
    }
}

// ─────────────────────────────── pack.mcmeta ───────────────────────────────────

/// `pack_format` según la versión. `None` (o una versión desconocida) → 4, el más
/// compatible: junto con `supported_formats` las versiones nuevas lo aceptan igual.
pub fn pack_format_for(version: Option<&str>) -> u32 {
    let Some(v) = version else { return 4 };
    let mut parts = v.trim().split('.');
    let major: u32 = match parts.next().and_then(|p| p.parse().ok()) {
        Some(n) => n,
        None => return 4,
    };
    let minor: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch: u32 = parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
    match (major, minor) {
        (1, 0..=8) => 1,
        (1, 9..=10) => 2,
        (1, 11..=12) => 3,
        (1, 13..=14) => 4,
        (1, 15) => 5,
        (1, 16) if patch <= 1 => 5,
        (1, 16) => 6,
        (1, 17) => 7,
        (1, 18) => 8,
        (1, 19) if patch <= 2 => 9,
        (1, 19) if patch == 3 => 12,
        (1, 19) => 13,
        (1, 20) if patch <= 1 => 15,
        (1, 20) if patch == 2 => 18,
        (1, 20) if patch <= 4 => 22,
        (1, 20) => 32,
        (1, 21) if patch >= 4 => 46,
        (1, 21) if patch >= 2 => 42,
        (1, 21) => 34,
        _ => 34,
    }
}

/// JSON de `pack.mcmeta`. `supported_formats` hace que 1.20.2+ acepte el pack sin
/// importar el `pack_format`; las versiones viejas solo miran `pack_format`.
pub fn pack_meta_json(version: Option<&str>) -> String {
    format!(
        "{{\"pack\":{{\"pack_format\":{},\"description\":\"Obsidia offline skin\",\"supported_formats\":{{\"min_inclusive\":1,\"max_inclusive\":999}}}}}}",
        pack_format_for(version)
    )
}

// ───────────────────────────── options.txt ─────────────────────────────────────

/// Extrae la lista de un renglón `clave:["a","b"]`. Si algo no parsea, devuelve vacío.
pub fn parse_pack_list(line: &str) -> Vec<String> {
    let (Some(start), Some(end)) = (line.find('['), line.rfind(']')) else {
        return Vec::new();
    };
    if start >= end {
        return Vec::new();
    }
    serde_json::from_str::<Vec<String>>(&line[start..=end]).unwrap_or_default()
}

/// Escribe el renglón `clave:[...]` con la lista exacta dada. Crea el renglón si falta.
pub fn set_pack_entries(options: &str, key: &str, packs: &[String]) -> String {
    let eol = if options.contains("\r\n") { "\r\n" } else { "\n" };
    let trailing = options.ends_with('\n');
    let mut lines: Vec<String> = if options.is_empty() {
        Vec::new()
    } else {
        options
            .split('\n')
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect()
    };
    if trailing {
        lines.pop();
    }
    let new_line = format!("{key}{}", serde_json::to_string(packs).unwrap_or_else(|_| "[]".into()));
    match lines.iter().position(|l| l.starts_with(key)) {
        Some(i) => lines[i] = new_line,
        None => lines.push(new_line),
    }
    let mut out = lines.join(eol);
    if trailing {
        out.push_str(eol);
    }
    out
}

/// Reescribe un renglón `clave:[...]`: agrega la entrada al principio (prioridad más alta)
/// o la quita. No toca las demás líneas ni sus saltos de línea. Si el renglón no existe
/// y hay que agregar, se crea al final.
pub fn edit_pack_list(options: &str, key: &str, entry: &str, enabled: bool) -> String {
    let line = options
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .find(|l| l.starts_with(key));
    // Quitar algo de un renglón que no existe no debe crear el renglón.
    if line.is_none() && !enabled {
        return options.to_string();
    }
    let mut packs = line.map(parse_pack_list).unwrap_or_default();
    packs.retain(|p| p != entry);
    if enabled {
        packs.insert(0, entry.to_string());
    }
    set_pack_entries(options, key, &packs)
}

/// Activa/desactiva el pack y lo saca de `incompatibleResourcePacks`
/// (Minecraft ignora un pack que figura como incompatible).
pub fn edit_resource_packs(options: &str, enabled: bool) -> String {
    let out = edit_pack_list(options, "resourcePacks:", PACK_ENTRY, enabled);
    edit_pack_list(&out, "incompatibleResourcePacks:", PACK_ENTRY, false)
}

// ─────────────────────────────── Aplicar / quitar ──────────────────────────────

fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Genera el resource pack y lo activa al principio de `options.txt`.
pub fn apply_skin(mc: &Path, png: &[u8], model: &str, version: Option<&str>) -> Result<(), String> {
    normalize_model(model)?;
    let pack = mc.join("resourcepacks").join(PACK_NAME);
    if pack.exists() {
        fs::remove_dir_all(&pack).map_err(|e| format!("{}: {e}", pack.display()))?;
    }

    write_if_changed(&pack.join("pack.mcmeta"), pack_meta_json(version).as_bytes())?;
    write_if_changed(&pack.join("pack.png"), png)?;

    let entity = pack.join("assets").join("minecraft").join("textures").join("entity");
    for name in PLAYER_SKINS {
        write_if_changed(&entity.join("player").join("wide").join(format!("{name}.png")), png)?;
        write_if_changed(&entity.join("player").join("slim").join(format!("{name}.png")), png)?;
    }
    // Versiones viejas (< 1.19.4) solo conocen estas dos.
    write_if_changed(&entity.join("steve.png"), png)?;
    write_if_changed(&entity.join("alex.png"), png)?;

    let options_path = mc.join("options.txt");
    let current = fs::read_to_string(&options_path).unwrap_or_default();
    let mut next = edit_resource_packs(&current, true);
    if !next.ends_with('\n') {
        next.push('\n');
    }
    fs::write(&options_path, next)
        .map_err(|e| format!("{}: {e}", options_path.display()))?;
    Ok(())
}

/// Borra el pack y saca la entrada de `options.txt`.
pub fn remove_skin(mc: &Path) -> Result<(), String> {
    let pack = mc.join("resourcepacks").join(PACK_NAME);
    if pack.exists() {
        fs::remove_dir_all(&pack).map_err(|e| format!("{}: {e}", pack.display()))?;
    }
    let options_path = mc.join("options.txt");
    if options_path.is_file() {
        let current = fs::read_to_string(&options_path).map_err(|e| e.to_string())?;
        fs::write(&options_path, edit_resource_packs(&current, false))
            .map_err(|e| format!("{}: {e}", options_path.display()))?;
    }
    Ok(())
}

// ─────────────────────────────────── Tests ─────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        let out = edit_resource_packs(options, true);
        assert!(out.contains("resourcePacks:[\"file/ObsidiaSkin\"]"));
        assert!(out.ends_with('\n'));
        assert!(out.starts_with("fov:0.5\nrenderDistance:12\n"));
    }

    #[test]
    fn packs_vacio() {
        let options = "resourcePacks:[]\n";
        let out = edit_resource_packs(options, true);
        assert_eq!(out, "resourcePacks:[\"file/ObsidiaSkin\"]\n");
    }

    #[test]
    fn packs_con_otros_al_principio() {
        let options = "resourcePacks:[\"file/MiPack.zip\",\"vanilla\"]\nincompatibleResourcePacks:[]\n";
        let out = edit_resource_packs(options, true);
        assert!(out.contains("resourcePacks:[\"file/ObsidiaSkin\",\"file/MiPack.zip\",\"vanilla\"]"));
        assert!(out.contains("incompatibleResourcePacks:[]"));
    }

    #[test]
    fn packs_ya_presente_no_duplica() {
        let options = "resourcePacks:[\"file/Otro\",\"file/ObsidiaSkin\"]\n";
        let out = edit_resource_packs(options, true);
        assert_eq!(out.matches("file/ObsidiaSkin").count(), 1);
        assert!(out.contains("resourcePacks:[\"file/ObsidiaSkin\",\"file/Otro\"]"));
    }

    #[test]
    fn packs_quitar() {
        let options = "resourcePacks:[\"file/ObsidiaSkin\",\"vanilla\"]\nincompatibleResourcePacks:[]\n";
        let out = edit_resource_packs(options, false);
        assert_eq!(out, "resourcePacks:[\"vanilla\"]\nincompatibleResourcePacks:[]\n");
    }

    #[test]
    fn packs_crlf() {
        let options = "fov:0.5\r\nresourcePacks:[]\r\n";
        let out = edit_resource_packs(options, true);
        assert_eq!(out, "fov:0.5\r\nresourcePacks:[\"file/ObsidiaSkin\"]\r\n");
    }

    #[test]
    fn options_113_y_121() {
        // 1.13 y 1.21 escriben la misma forma de línea.
        let viejo = "resourcePacks:[]\nincompatibleResourcePacks:[]\n";
        let nuevo = "resourcePacks:[\"vanilla\",\"file/pack.zip\"]\nincompatibleResourcePacks:[\"file/viejo.zip\"]\n";
        let a = edit_resource_packs(viejo, true);
        let b = edit_resource_packs(nuevo, true);
        assert!(a.contains("resourcePacks:[\"file/ObsidiaSkin\"]"));
        assert!(b.contains("resourcePacks:[\"file/ObsidiaSkin\",\"vanilla\",\"file/pack.zip\"]"));
        assert!(b.contains("incompatibleResourcePacks:[\"file/viejo.zip\"]"));

        assert_eq!(pack_format_for(Some("1.13")), 4);
        assert_eq!(pack_format_for(Some("1.13.2")), 4);
        assert_eq!(pack_format_for(Some("1.21")), 34);
        assert_eq!(pack_format_for(Some("1.21.4")), 46);
        assert!(pack_meta_json(Some("1.13")).contains("\"pack_format\":4"));
        assert!(pack_meta_json(Some("1.21.4")).contains("\"pack_format\":46"));
        assert!(pack_meta_json(None).contains("\"max_inclusive\":999"));
    }
}
