//! Skin en modo offline.
//!
//! Minecraft no deja cambiar la skin de una cuenta offline (siempre usa Steve/Alex),
//! así que Obsidia genera un resource pack que reemplaza las texturas del jugador y lo
//! activa en `options.txt`. Solo la ve el jugador: el servidor sigue mostrando Steve.
//!
//! En `options.txt` la lista `resourcePacks` está en orden ascendente de prioridad:
//! la última entrada es la que gana. Por eso ObsidiaSkin se agrega al final.

use base64::Engine;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Nombre de la carpeta dentro de `resourcepacks`.
const PACK_NAME: &str = "ObsidiaSkin";
/// Entrada tal como la escribe Minecraft en `options.txt`.
const PACK_ENTRY: &str = "file/ObsidiaSkin";
/// Clave de los packs activos.
const RESOURCE_PACKS_KEY: &str = "resourcePacks:";
/// Clave de los packs marcados como incompatibles (Minecraft los ignora).
const INCOMPATIBLE_KEY: &str = "incompatibleResourcePacks:";
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

/// Repara los restos de una escritura interrumpida de `options.txt`.
///
/// La aplicación nunca borra `options.txt`: si falta y existe un `.bak`, ese respaldo
/// es el último contenido bueno y se restaura. Si existen los dos, el `.bak` es de una
/// ejecución anterior y se archiva con un nombre único: nunca se pisa ni se borra.
pub fn recover_orphan_backup(path: &Path) -> Result<(), String> {
    let backup = with_suffix(path, ".bak");
    if !backup.exists() {
        return Ok(());
    }
    if path.exists() {
        archive_backup(&backup)?;
        return Ok(());
    }
    fs::rename(&backup, path).map_err(|e| {
        format!(
            "{}: {e}. Conservé el respaldo en {}",
            path.display(),
            backup.display()
        )
    })
}

/// Mueve un respaldo a `*.bak.N`, con el primer número libre. Nunca sobrescribe una
/// copia anterior.
fn archive_backup(backup: &Path) -> Result<PathBuf, String> {
    for n in 1..=u32::MAX {
        let candidate = with_suffix(backup, &format!(".{n}"));
        if candidate.exists() {
            continue;
        }
        fs::rename(backup, &candidate).map_err(|e| format!("{}: {e}", backup.display()))?;
        return Ok(candidate);
    }
    Err(format!(
        "{}: no encontré un nombre libre para archivarlo",
        backup.display()
    ))
}

/// Lee `options.txt` como texto.
///
/// Antes de leer recupera o archiva un `.bak` huérfano. Un archivo inexistente (sin
/// respaldo) devuelve texto vacío: primera ejecución. Cualquier otro error (permisos,
/// es un directorio, bytes que no son UTF-8) se propaga: el llamador NO debe
/// sobrescribir el archivo en ese caso.
pub fn read_options(path: &Path) -> Result<String, String> {
    recover_orphan_backup(path)?;
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| format!("{}: no es texto UTF-8 válido", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Último renglón que empieza con `key`. Si hay claves duplicadas, Minecraft se queda
/// con la última, así que es la única que hay que leer y reescribir.
pub fn find_pack_line<'a>(options: &'a str, key: &str) -> Option<&'a str> {
    options
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .rfind(|l| l.starts_with(key))
}

/// Extrae la lista de un renglón `clave:["a","b"]`.
///
/// Si el renglón existe pero el JSON no es válido devuelve error: nunca se toma como
/// lista vacía, porque el siguiente guardado borraría los packs del jugador.
pub fn parse_pack_list(line: &str) -> Result<Vec<String>, String> {
    let (Some(start), Some(end)) = (line.find('['), line.rfind(']')) else {
        return Err(format!("{line:?}: no tiene una lista de resource packs"));
    };
    if start >= end {
        return Err(format!("{line:?}: no tiene una lista de resource packs"));
    }
    serde_json::from_str::<Vec<String>>(&line[start..=end])
        .map_err(|e| format!("{line:?}: lista de resource packs inválida ({e})"))
}

/// Escribe el renglón `clave:[...]` con la lista exacta dada. Crea el renglón si falta.
///
/// Conserva el formato original: el mismo salto de línea (CRLF o LF) y la ausencia de
/// salto final si el archivo no lo tenía. Si la clave está duplicada reescribe la
/// última, que es la que Minecraft usa.
pub fn set_pack_entries(options: &str, key: &str, packs: &[String]) -> Result<String, String> {
    let eol = if options.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
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
    let new_line = format!(
        "{key}{}",
        serde_json::to_string(packs).map_err(|e| e.to_string())?
    );
    match lines.iter().rposition(|l| l.starts_with(key)) {
        Some(i) => lines[i] = new_line,
        None => lines.push(new_line),
    }
    let mut out = lines.join(eol);
    if trailing {
        out.push_str(eol);
    }
    Ok(out)
}

/// Reescribe un renglón `clave:[...]`: agrega la entrada al final (prioridad más alta)
/// o la quita. No toca las demás líneas ni sus saltos de línea. Si el renglón no existe
/// y hay que agregar, se crea al final.
pub fn edit_pack_list(
    options: &str,
    key: &str,
    entry: &str,
    enabled: bool,
) -> Result<String, String> {
    let line = find_pack_line(options, key);
    // Quitar algo de un renglón que no existe no debe crear el renglón.
    if line.is_none() && !enabled {
        return Ok(options.to_string());
    }
    let mut packs = match line {
        Some(l) => parse_pack_list(l)?,
        None => Vec::new(),
    };
    let before = packs.clone();
    packs.retain(|p| p != entry);
    if enabled {
        packs.push(entry.to_string());
    }
    if packs == before {
        return Ok(options.to_string());
    }
    set_pack_entries(options, key, &packs)
}

/// Activa/desactiva el pack y lo saca de `incompatibleResourcePacks`
/// (Minecraft ignora un pack que figura como incompatible).
pub fn edit_resource_packs(options: &str, enabled: bool) -> Result<String, String> {
    let out = edit_pack_list(options, RESOURCE_PACKS_KEY, PACK_ENTRY, enabled)?;
    edit_pack_list(&out, INCOMPATIBLE_KEY, PACK_ENTRY, false)
}

/// Escribe `options.txt` con un temporal en el mismo directorio y un respaldo `.bak`.
///
/// NO es atómico: son varios pasos (recuperar o archivar un `.bak` previo, crear y
/// sincronizar el temporal, mover el original a `.bak`, mover el temporal a su lugar).
/// Si el reemplazo falla se intenta restaurar el original; si la restauración también
/// falla, el `.bak` se conserva para recuperarlo a mano.
///
/// Al terminar bien, el `.bak` queda con la versión anterior de `options.txt`. No se
/// borra: una lectura o escritura posterior lo archiva como `*.bak.N` sin pisar copias
/// anteriores.
pub fn write_options(path: &Path, data: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    recover_orphan_backup(path)?;

    let tmp = with_suffix(path, ".tmp");
    let backup = with_suffix(path, ".bak");

    {
        let mut file = fs::File::create(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        file.write_all(data.as_bytes())
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
        // El temporal tiene que estar completo antes de tocar el original.
        file.sync_all()
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
    }

    let had_original = path.exists();
    if had_original {
        // La recuperación anterior dejó libre el nombre `.bak`.
        if let Err(e) = fs::rename(path, &backup) {
            let _ = fs::remove_file(&tmp);
            return Err(format!("{}: {e}", path.display()));
        }
    }

    if let Err(e) = fs::rename(&tmp, path) {
        if had_original && backup.exists() && fs::rename(&backup, path).is_err() {
            return Err(format!(
                "{}: {e}. No pude restaurar el original; quedó el respaldo en {}",
                path.display(),
                backup.display()
            ));
        }
        let _ = fs::remove_file(&tmp);
        return Err(format!("{}: {e}", path.display()));
    }

    Ok(())
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

// ─────────────────────────────── Aplicar / quitar ──────────────────────────────

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

    write_file(&tmp.join("pack.mcmeta"), pack_meta_json(version).as_bytes())?;
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
    let current = read_options(&options_path)?;
    let next = edit_resource_packs(&current, true)?;

    let packs = mc.join("resourcepacks");
    if crate::paths::is_reparse_point(&packs) {
        return Err(format!(
            "{} no puede ser un junction ni un symlink",
            packs.display()
        ));
    }

    write_pack(mc, png, version)?;

    if next != current {
        write_options(&options_path, &next)?;
    }
    Ok(())
}

/// Saca la entrada de `options.txt` y después borra el pack.
///
/// Ese orden evita dejar una entrada que apunte a una carpeta ya borrada: si el pack
/// no se puede borrar, como mucho queda una carpeta sin usar.
pub fn remove_skin(mc: &Path) -> Result<(), String> {
    let options_path = mc.join("options.txt");
    let current = read_options(&options_path)?;
    let next = edit_resource_packs(&current, false)?;
    if next != current {
        write_options(&options_path, &next)?;
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
        assert!(pack_meta_json(None).contains("\"max_inclusive\":999"));
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
