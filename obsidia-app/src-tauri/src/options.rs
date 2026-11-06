//! Lectura y escritura de `options.txt` de Minecraft.
//!
//! Contiene las funciones de opciones de packs y el manejo de respaldos.
//! Se movieron aquí desde `skin.rs` para que `launcher.rs` y `packs.rs`
//! puedan usarlas sin depender de `skin.rs`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Clave de los packs activos.
pub const RESOURCE_PACKS_KEY: &str = "resourcePacks:";
/// Clave de los packs marcados como incompatibles (Minecraft los ignora).
pub const INCOMPATIBLE_KEY: &str = "incompatibleResourcePacks:";
/// Entrada del pack de skin de Obsidia.
pub const PACK_ENTRY: &str = "file/ObsidiaSkin";

/// Repara los restos de una escritura interrumpida de `options.txt`.
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

/// Mueve un respaldo a `*.bak.N`, con el primer número libre.
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
pub fn read_options(path: &Path) -> Result<String, String> {
    recover_orphan_backup(path)?;
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| format!("{}: no es texto UTF-8 válido", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Último renglón que empieza con `key`.
pub fn find_pack_line<'a>(options: &'a str, key: &str) -> Option<&'a str> {
    options
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .rfind(|l| l.starts_with(key))
}

/// Extrae la lista de un renglón `clave:["a","b"]`.
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

/// Escribe el renglón `clave:[...]` con la lista exacta dada.
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

/// Reescribe un renglón `clave:[...]`: agrega la entrada al final o la quita.
pub fn edit_pack_list(
    options: &str,
    key: &str,
    entry: &str,
    enabled: bool,
) -> Result<String, String> {
    let line = find_pack_line(options, key);
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

/// Activa/desactiva el pack de ObsidiaSkin en options.txt.
pub fn edit_resource_packs(options: &str, enabled: bool) -> Result<String, String> {
    let out = edit_pack_list(options, RESOURCE_PACKS_KEY, PACK_ENTRY, enabled)?;
    edit_pack_list(&out, INCOMPATIBLE_KEY, PACK_ENTRY, false)
}

/// Escribe `options.txt` con temporal + respaldo `.bak`.
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
        file.sync_all()
            .map_err(|e| format!("{}: {e}", tmp.display()))?;
    }

    let had_original = path.exists();
    if had_original && let Err(e) = fs::rename(path, &backup) {
        let _ = fs::remove_file(&tmp);
        return Err(format!("{}: {e}", path.display()));
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

/// Escribe el renglón `clave:valor` con el valor exacto dado. Crea el renglón si falta.
///
/// Conserva el formato original: el mismo salto de línea (CRLF o LF) y la ausencia de
/// salto final si el archivo no lo tenía. Si la clave está duplicada reescribe la
/// última, que es la que Minecraft usa.
pub fn set_entry(options: &str, key: &str, value: &str) -> String {
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
    let new_line = format!("{key}{value}");
    match lines.iter().rposition(|l| l.starts_with(key)) {
        Some(i) => lines[i] = new_line,
        None => lines.push(new_line),
    }
    let mut out = lines.join(eol);
    if trailing {
        out.push_str(eol);
    }
    out
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}
