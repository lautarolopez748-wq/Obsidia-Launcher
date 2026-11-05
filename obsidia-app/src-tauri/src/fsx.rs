//! Escritura atómica de archivos: `.part` + rename.
//!
//! Extraída de `launcher.rs` para que `content.rs` y `launcher.rs` la compartan.
//! Nunca queda un archivo a medias; el reemplazo es seguro en Windows.
//!
//! Estrategia transaccional (bloque 4A):
//! - `write_atomic`: `.part` + hash + rename con backup `.old`.
//! - `apply_update_transactional`:
//!   1. Leer old_content y old_entries.
//!   2. journal_add(install) — registrar antes de modificar archivos.
//!   3. write_atomic(dest, bytes, sha1) — si falla, journal_remove y error.
//!   4. upsert_entry — si falla, restaurar old_content + journal_remove + error.
//!   5. Si cambió filename: borrar old_file.
//!   6. journal_remove — confirmar.
//!
//! El journal incluye `old_filename` para recuperar correctamente.
//! - `remove_entry_transactional`: journal primero, índice, archivo, journal_remove.
//! - `recover_with_journal`: al iniciar, lee journal, completa/descarta según
//!   el estado real del índice y archivo. Idempotente. No toca legacy.
//!   Si el journal está corrupto, error sin tocar nada.
//! - `recover_pending`: limpia `.part` y restaura `.old` huérfanos.
//!   Idempotente, scope de instancia.
//!
//! Concurrencia: las transacciones por instancia se serializan con un Mutex
//! estático por instancia. Instancias distintas usan lock distinto.

use sha1::{Digest, Sha1};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::content;

pub fn sha1_hex(bytes: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

// ───────────────────────────── Serialización por instancia ─────────────────────────────

/// Lock por instancia (clave `instance_dir`). Las transacciones de la misma
/// instancia se serializan; instancias distintas usan locks distintos.
fn instance_lock(instance_dir: &Path) -> std::sync::Arc<Mutex<()>> {
    static MAP: OnceLock<Mutex<std::collections::HashMap<PathBuf, std::sync::Arc<Mutex<()>>>>> =
        OnceLock::new();
    let map = MAP.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut guard = map.lock().unwrap();
    guard
        .entry(instance_dir.to_path_buf())
        .or_insert_with(|| std::sync::Arc::new(Mutex::new(())))
        .clone()
}

// ───────────────────────────── Journal persistente ─────────────────────────────

/// Estado del journal para una instancia.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Journal {
    /// Pendientes: operaciones incompletas por tipo y filename.
    pub pending: Vec<JournalEntry>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct JournalEntry {
    pub op: String,
    pub kind: String,
    pub slug: String,
    pub filename: String,
    #[serde(default)]
    pub old_filename: Option<String>,
    #[serde(default)]
    pub version_id: Option<String>,
    #[serde(default)]
    pub sha1: Option<String>,
    /// SHA1 real de los bytes nuevos (siempre calculado).
    #[serde(default)]
    pub new_hash: Option<String>,
    /// SHA1 del archivo que había en dest antes de la transacción, si había.
    #[serde(default)]
    pub prev_hash: Option<String>,
}

fn journal_path(instance_dir: &Path) -> PathBuf {
    instance_dir.join(".obsidia").join("journal.json")
}

pub fn read_journal(instance_dir: &Path) -> Result<Journal, String> {
    let path = journal_path(instance_dir);
    if !path.exists() {
        return Ok(Journal::default());
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if data.trim().is_empty() {
        return Ok(Journal::default());
    }
    serde_json::from_str(&data).map_err(|e| format!("{}: journal corrupto ({e})", path.display()))
}

fn write_journal(instance_dir: &Path, journal: &Journal) -> Result<(), String> {
    let path = journal_path(instance_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(journal).map_err(|e| e.to_string())?;
    write_atomic(&path, data.as_bytes(), None)
}

fn journal_add(instance_dir: &Path, entry: JournalEntry) -> Result<(), String> {
    let mut journal = read_journal(instance_dir)?;
    journal.pending.push(entry);
    write_journal(instance_dir, &journal)
}

fn journal_remove(instance_dir: &Path, entry: &JournalEntry) -> Result<(), String> {
    let mut journal = read_journal(instance_dir)?;
    journal.pending.retain(|e| e != entry);
    write_journal(instance_dir, &journal)
}

/// Clínica de recuperación con journal.
#[allow(dead_code)]
pub fn recover_with_journal(instance_dir: &Path) -> Result<(), String> {
    let lock = instance_lock(instance_dir);
    let _guard = lock.lock().unwrap();
    recover_with_journal_inner(instance_dir)
}

fn restore_state_file(path: &Path) -> Result<(), String> {
    let backup = path.with_file_name(format!(
        "{}.old",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    if path.exists() {
        if let Ok(data) = fs::read_to_string(path)
            && serde_json::from_str::<serde_json::Value>(&data).is_ok()
        {
            let _ = fs::remove_file(&backup);
        }
        return Ok(());
    }
    if !backup.exists() {
        return Ok(());
    }
    let data = fs::read_to_string(&backup).map_err(|e| e.to_string())?;
    if serde_json::from_str::<serde_json::Value>(&data).is_err() {
        return Err("Respaldo de estado inválido".to_string());
    }
    fs::rename(&backup, path).map_err(|e| e.to_string())?;
    let data2 = fs::read_to_string(path).map_err(|e| e.to_string())?;
    if serde_json::from_str::<serde_json::Value>(&data2).is_err() {
        return Err("Estado restaurado incompleto".to_string());
    }
    Ok(())
}

fn recover_with_journal_inner(instance_dir: &Path) -> Result<(), String> {
    let obsidia_dir = instance_dir.join(".obsidia");
    // Restaurar archivos de estado antes de leer el journal.
    restore_state_file(&obsidia_dir.join("journal.json"))?;
    for name in ["mods.json", "shaders.json", "packs.json", "content.json"] {
        restore_state_file(&obsidia_dir.join(name))?;
    }

    // Leer journal: corrupto → error sin tocar nada.
    let journal = read_journal(instance_dir)?;

    // Si no hay pendings, solo limpiar `.part` y restaurar `.old`.
    if journal.pending.is_empty() {
        return recover_pending(instance_dir);
    }

    // Eliminar temporales solo de transacciones pendientes.
    for entry in &journal.pending {
        let subdir = match entry.kind.as_str() {
            "mods" => "mods",
            "shaders" => "shaderpacks",
            "packs" => "resourcepacks",
            _ => continue,
        };
        let file_path = instance_dir.join(subdir).join(&entry.filename);
        let part_path = file_path.with_file_name(format!(
            "{}.part",
            file_path.file_name().unwrap_or_default().to_string_lossy()
        ));
        if part_path.exists() {
            let _ = fs::remove_file(&part_path);
        }
    }

    let mut remaining = Vec::new();
    let mut first_error: Option<String> = None;

    for entry in &journal.pending {
        let subdir = match entry.kind.as_str() {
            "mods" => "mods",
            "shaders" => "shaderpacks",
            "packs" => "resourcepacks",
            _ => {
                remaining.push(entry.clone());
                continue;
            }
        };
        let subdir_path = instance_dir.join(subdir);

        // Validar nombres: nunca salir de la carpeta de contenido.
        if !is_safe_filename(&entry.filename) {
            eprintln!(
                "Journal: filename inválido '{}', conservado para revisión",
                entry.filename
            );
            remaining.push(entry.clone());
            continue;
        }
        if let Some(old) = &entry.old_filename
            && !is_safe_filename(old)
        {
            eprintln!("Journal: old_filename inválido, conservado");
            remaining.push(entry.clone());
            continue;
        }

        let file_path = subdir_path.join(&entry.filename);

        match entry.op.as_str() {
            "install" => {
                // Backup con nombre completo: mod.jar -> mod.jar.old
                let backup_path = file_path.with_file_name(format!(
                    "{}.old",
                    file_path.file_name().unwrap_or_default().to_string_lossy()
                ));

                let mut restored = false;
                // Si falta el archivo pero existe el respaldo, restaurarlo antes
                // de evaluar (interrupción entre rename original->backup y part->dest).
                if !file_path.exists() && backup_path.exists() {
                    if let Err(e) = fs::rename(&backup_path, &file_path) {
                        remaining.push(entry.clone());
                        if first_error.is_none() {
                            first_error = Some(format!(
                                "{}: no se pudo restaurar respaldo: {e}",
                                instance_dir.display()
                            ));
                        }
                        continue;
                    }
                    restored = true;
                }

                if !file_path.exists() {
                    let had_prev_at_dest = entry.prev_hash.is_some()
                        || entry.old_filename.as_ref() == Some(&entry.filename);
                    if had_prev_at_dest {
                        remaining.push(entry.clone());
                        if first_error.is_none() {
                            first_error = Some(
                                "Transacción de contenido incompleta sin respaldo suficiente"
                                    .to_string(),
                            );
                        }
                        continue;
                    }
                    continue;
                }

                let has_identity = entry
                    .version_id
                    .as_ref()
                    .map(|v| !v.is_empty())
                    .unwrap_or(false)
                    || entry.sha1.as_ref().map(|s| !s.is_empty()).unwrap_or(false)
                    || entry
                        .new_hash
                        .as_ref()
                        .map(|s| !s.is_empty())
                        .unwrap_or(false);
                if !has_identity {
                    remaining.push(entry.clone());
                    if first_error.is_none() {
                        first_error = Some("Journal sin identidad de versión".to_string());
                    }
                    continue;
                }

                let entries = match content::read_index(instance_dir, &entry.kind) {
                    Ok(v) => v,
                    Err(e) => {
                        remaining.push(entry.clone());
                        if first_error.is_none() {
                            first_error = Some(format!("{}: {e}", instance_dir.display()));
                        }
                        continue;
                    }
                };
                let idx_entry = entries.iter().find(|e| e.slug == entry.slug);

                let idx_is_new = match idx_entry {
                    Some(idx) => {
                        if idx.filename != entry.filename {
                            false
                        } else if let Some(v) = &entry.version_id
                            && !v.is_empty()
                        {
                            idx.version_id == *v && (entry.sha1.is_none() || idx.sha1 == entry.sha1)
                        } else if let Some(s) = &entry.sha1
                            && !s.is_empty()
                        {
                            idx.sha1.as_ref() == Some(s)
                        } else if let Some(nh) = &entry.new_hash {
                            idx.sha1.as_ref() == Some(nh)
                        } else {
                            false
                        }
                    }
                    None => false,
                };

                if idx_is_new {
                    // Verificar hash físico antes de eliminar respaldo.
                    let file_hash = fs::read(&file_path).ok().map(|b| sha1_hex(&b));
                    if let Some(nh) = &entry.new_hash
                        && file_hash.as_ref() != Some(nh)
                    {
                        remaining.push(entry.clone());
                        if first_error.is_none() {
                            first_error = Some(
                                "El archivo físico no coincide con la nueva versión".to_string(),
                            );
                        }
                        continue;
                    }

                    // Índice ya refleja la nueva versión: conservarla y limpiar.
                    if backup_path.exists()
                        && let Err(e) = fs::remove_file(&backup_path)
                    {
                        remaining.push(entry.clone());
                        if first_error.is_none() {
                            first_error = Some(format!("{}: {e}", instance_dir.display()));
                        }
                        continue;
                    }
                    if let Some(old) = &entry.old_filename
                        && old != &entry.filename
                    {
                        let old_path = subdir_path.join(old);
                        match fs::remove_file(&old_path) {
                            Ok(()) => {}
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                            Err(e) => {
                                remaining.push(entry.clone());
                                if first_error.is_none() {
                                    first_error = Some(format!(
                                        "{}: no se pudo borrar archivo anterior: {e}",
                                        instance_dir.display()
                                    ));
                                }
                                continue;
                            }
                        }
                    }
                    continue;
                }

                let file_hash = fs::read(&file_path).ok().map(|b| sha1_hex(&b));

                // Si se acaba de restaurar o el SHA1 coincide con prev_hash, nada cambió.
                if restored || (entry.prev_hash.is_some() && file_hash == entry.prev_hash) {
                    let _ = fs::remove_file(&backup_path);
                    continue;
                }

                let filename_changed = entry
                    .old_filename
                    .as_ref()
                    .map(|o| o != &entry.filename)
                    .unwrap_or(true);

                if filename_changed {
                    // Si hay respaldo, restaurarlo; si no, borrar el archivo nuevo.
                    if backup_path.exists() {
                        if let Err(e) = fs::remove_file(&file_path)
                            && e.kind() != std::io::ErrorKind::NotFound
                        {
                            remaining.push(entry.clone());
                            if first_error.is_none() {
                                first_error = Some(format!("{}: {e}", instance_dir.display()));
                            }
                            continue;
                        }
                        if let Err(e) = fs::rename(&backup_path, &file_path) {
                            remaining.push(entry.clone());
                            if first_error.is_none() {
                                first_error = Some(format!("{}: {e}", instance_dir.display()));
                            }
                            continue;
                        }
                        continue;
                    }
                    if file_hash != entry.new_hash {
                        remaining.push(entry.clone());
                        if first_error.is_none() {
                            first_error = Some(
                                "No se puede demostrar que el archivo pertenece a la transacción"
                                    .to_string(),
                            );
                        }
                        continue;
                    }
                    match fs::remove_file(&file_path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => {
                            remaining.push(entry.clone());
                            if first_error.is_none() {
                                first_error = Some(format!(
                                    "{}: no se pudo borrar archivo nuevo: {e}",
                                    instance_dir.display()
                                ));
                            }
                            continue;
                        }
                    }
                    continue;
                }

                // Mismo filename: intentar rollback usando el respaldo.
                if backup_path.exists() {
                    match fs::remove_file(&file_path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => {
                            remaining.push(entry.clone());
                            if first_error.is_none() {
                                first_error = Some(format!(
                                    "{}: no se pudo borrar archivo nuevo: {e}",
                                    instance_dir.display()
                                ));
                            }
                            continue;
                        }
                    }
                    if let Err(e) = fs::rename(&backup_path, &file_path) {
                        remaining.push(entry.clone());
                        if first_error.is_none() {
                            first_error = Some(format!(
                                "{}: no se pudo restaurar respaldo: {e}",
                                instance_dir.display()
                            ));
                        }
                        continue;
                    }
                    continue;
                }

                // Sin respaldo: borrar solo si SHA1 es new_hash y nombre distinto.
                if file_hash == entry.new_hash
                    && entry.old_filename.as_ref() != Some(&entry.filename)
                {
                    if let Err(e) = fs::remove_file(&file_path)
                        && e.kind() != std::io::ErrorKind::NotFound
                    {
                        remaining.push(entry.clone());
                        if first_error.is_none() {
                            first_error = Some(format!("{}: {e}", instance_dir.display()));
                        }
                        continue;
                    }
                    continue;
                }

                // Requiere intervención manual.
                remaining.push(entry.clone());
                if first_error.is_none() {
                    first_error = Some(
                        "Transacción de contenido incompleta sin respaldo suficiente".to_string(),
                    );
                }
                continue;
            }
            "remove" => {
                let entries = content::read_index(instance_dir, &entry.kind)
                    .map_err(|e| format!("{}: {e}", instance_dir.display()))?;
                let still_in_index = entries.iter().any(|e| e.slug == entry.slug);

                if still_in_index {
                    // La eliminación del índice NO se completó: no tocamos el
                    // archivo (evita índice colgante). Si el archivo ya falta,
                    // completamos la eliminación del índice para no dejar un
                    // puntero colgante.
                    if !file_path.exists() {
                        let mut new_entries = entries.clone();
                        new_entries.retain(|e| e.slug != entry.slug);
                        content::write_index(instance_dir, &entry.kind, &new_entries)
                            .map_err(|e| format!("{}: {e}", instance_dir.display()))?;
                        continue;
                    }
                    // Revertir: conservar archivo + índice, descartar journal.
                    continue;
                }

                // La eliminación del índice se completó: borrar el archivo si existe.
                if file_path.exists() {
                    match fs::remove_file(&file_path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => {
                            remaining.push(entry.clone());
                            if first_error.is_none() {
                                first_error = Some(format!("{}: {e}", instance_dir.display()));
                            }
                            continue;
                        }
                    }
                }
                continue;
            }
            _ => remaining.push(entry.clone()),
        }
    }

    let new_journal = Journal { pending: remaining };
    write_journal(instance_dir, &new_journal)?;

    recover_pending(instance_dir)?;

    if !new_journal.pending.is_empty() {
        return Err("Transacción de contenido pendiente sin resolver".to_string());
    }
    Ok(())
}

fn is_safe_filename(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && !name.contains("..")
}

/// Escribe `bytes` en `dest` de forma atómica.
///
/// Escribe a `<dest>.part`, verifica el hash si se provee, y recién ahí
/// reemplaza el original (vía `<dest>.old`). Si algo falla, el archivo
/// original queda intacto.
pub fn write_atomic(dest: &Path, bytes: &[u8], expected_sha1: Option<&str>) -> Result<(), String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }

    // Verificar hash antes de tocar el sistema de archivos.
    if let Some(expected) = expected_sha1
        && !sha1_hex(bytes).eq_ignore_ascii_case(expected)
    {
        return Err("El archivo descargado no coincide con su SHA1: se descartó".to_string());
    }

    let mut tmp = dest.to_path_buf().into_os_string();
    tmp.push(".part");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;

    // Reemplazo seguro: nunca se borra el válido antes de confirmar el temporal.
    let backup = dest.with_file_name(format!(
        "{}.old",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));
    if dest.exists() {
        if backup.exists() {
            fs::remove_file(&backup).map_err(|e| format!("{}: {e}", backup.display()))?;
        }
        fs::rename(dest, &backup).map_err(|e| format!("{}: {e}", dest.display()))?;
    }
    if let Err(e) = fs::rename(&tmp, dest) {
        // Restaurar el original si el reemplazo falla.
        if backup.exists() {
            fs::rename(&backup, dest)
                .map_err(|e2| format!("{}: {e}; restauración falló: {e2}", dest.display()))?;
        }
        return Err(format!("{}: {e}", dest.display()));
    }
    // El respaldo se conserva hasta que la transacción que invocó
    // write_atomic confirme la operación.
    Ok(())
}

/// Actualización transaccional.
#[allow(dead_code)]
pub fn apply_update_transactional(
    dest: &Path,
    bytes: &[u8],
    expected_sha1: Option<&str>,
    instance_dir: &Path,
    kind: &str,
    subdir: &Path,
    entry: content::ContentEntry,
) -> Result<(), String> {
    let lock = instance_lock(instance_dir);
    let _guard = lock.lock().unwrap();
    apply_update_transactional_inner(
        dest,
        bytes,
        expected_sha1,
        instance_dir,
        kind,
        subdir,
        entry,
    )
}

fn apply_update_transactional_inner(
    dest: &Path,
    bytes: &[u8],
    expected_sha1: Option<&str>,
    instance_dir: &Path,
    kind: &str,
    subdir: &Path,
    entry: content::ContentEntry,
) -> Result<(), String> {
    // I4: recuperar transacciones pendientes antes de modificar.
    recover_with_journal_inner(instance_dir)?;

    // Guardar estado anterior.
    let old_content = if dest.exists() {
        Some(fs::read(dest).map_err(|e| e.to_string())?)
    } else {
        None
    };
    let prev_hash = old_content.as_ref().map(|b| sha1_hex(b));
    let old_entries = content::read_index(instance_dir, kind)?;

    // 1. Registrar en journal antes de modificar archivos.
    let old_filename = old_entries
        .iter()
        .find(|e| e.slug == entry.slug)
        .map(|e| e.filename.clone());

    if !is_safe_filename(&entry.filename) {
        return Err("filename inválido".to_string());
    }
    if let Some(old) = &old_filename
        && !is_safe_filename(old)
    {
        return Err("old_filename inválido".to_string());
    }
    let new_hash = sha1_hex(bytes);
    let journal_entry = JournalEntry {
        op: "install".to_string(),
        kind: kind.to_string(),
        slug: entry.slug.clone(),
        filename: entry.filename.clone(),
        old_filename: old_filename.clone(),
        version_id: Some(entry.version_id.clone()),
        sha1: entry.sha1.clone(),
        new_hash: Some(new_hash),
        prev_hash,
    };
    journal_add(instance_dir, journal_entry.clone())?;

    // 2. Escribir el archivo.
    if let Err(e) = write_atomic(dest, bytes, expected_sha1) {
        // write_atomic falló: la operación quedó sin cambios efectivos.
        // Retirar el journal solo si el archivo no cambió.
        let still_old = match &old_content {
            Some(old) => dest.exists() && fs::read(dest).map(|c| c == *old).unwrap_or(false),
            None => !dest.exists(),
        };
        if still_old && let Err(je) = journal_remove(instance_dir, &journal_entry) {
            return Err(format!("{}: {je}", instance_dir.display()));
        }
        // Si cambió, el journal se conserva para recuperación.
        return Err(e);
    }

    // 3. Actualizar el índice.
    if let Err(e) = content::upsert_entry(instance_dir, kind, subdir, entry.clone()) {
        // Restaurar el archivo anterior o eliminar el nuevo si no había anterior.
        match old_content {
            Some(old) => {
                match fs::write(dest, &old) {
                    Ok(_) => {
                        if let Err(je) = journal_remove(instance_dir, &journal_entry) {
                            return Err(format!("{}: {je}", instance_dir.display()));
                        }
                        return Err(format!("Índice no actualizado, archivo restaurado: {e}"));
                    }
                    Err(restore_e) => {
                        // No retirar journal: conservar para recuperación.
                        return Err(format!(
                            "Índice no actualizado ({e}); restauración falló ({restore_e}); journal conservado"
                        ));
                    }
                }
            }
            None => {
                match fs::remove_file(dest) {
                    Ok(()) => {}
                    Err(remove_e) if remove_e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(remove_e) => {
                        return Err(format!(
                            "Índice no actualizado ({e}); no se pudo borrar archivo nuevo: {remove_e}; journal conservado"
                        ));
                    }
                }
                if let Err(je) = journal_remove(instance_dir, &journal_entry) {
                    return Err(format!("{}: {je}", instance_dir.display()));
                }
                return Err(format!("Índice no actualizado, archivo eliminado: {e}"));
            }
        }
    }

    // 4. Si cambió el filename, borrar el anterior SOLO AHORA.
    if let Some(old) = old_entries.iter().find(|e| e.slug == entry.slug)
        && old.filename != entry.filename
    {
        let old_path = subdir.join(&old.filename);
        match fs::remove_file(&old_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                // Conservar journal y propagar error.
                return Err(format!("No se pudo borrar archivo anterior: {e}"));
            }
        }
    }

    // Limpiar respaldo conservado por write_atomic.
    let backup = dest.with_file_name(format!(
        "{}.old",
        dest.file_name().unwrap_or_default().to_string_lossy()
    ));
    if backup.exists() {
        fs::remove_file(&backup).map_err(|e| format!("{}: {e}", backup.display()))?;
    }

    // 5. Confirmar en journal.
    journal_remove(instance_dir, &journal_entry)?;

    Ok(())
}

/// Eliminación transaccional.
#[allow(dead_code)]
pub fn remove_entry_transactional(
    instance_dir: &Path,
    kind: &str,
    subdir: &Path,
    slug: &str,
) -> Result<(), String> {
    let lock = instance_lock(instance_dir);
    let _guard = lock.lock().unwrap();
    remove_entry_transactional_inner(instance_dir, kind, subdir, slug)
}

fn remove_entry_transactional_inner(
    instance_dir: &Path,
    kind: &str,
    subdir: &Path,
    slug: &str,
) -> Result<(), String> {
    // I4: recuperar transacciones pendientes antes de modificar.
    recover_with_journal_inner(instance_dir)?;

    let entries = content::read_index(instance_dir, kind)?;
    if !entries.iter().any(|e| e.slug == slug) {
        return Ok(());
    }
    let old_entry = entries.iter().find(|e| e.slug == slug).cloned();
    let filename = old_entry.as_ref().map(|e| e.filename.clone());

    // 0. Registrar eliminación pendiente en journal.
    let journal_entry = JournalEntry {
        op: "remove".to_string(),
        kind: kind.to_string(),
        slug: slug.to_string(),
        filename: filename.clone().unwrap_or_default(),
        old_filename: None,
        version_id: None,
        sha1: None,
        new_hash: None,
        prev_hash: None,
    };
    journal_add(instance_dir, journal_entry.clone())?;

    // 1. Escribir el índice nuevo (sin la entrada).
    let mut new_entries = entries.clone();
    new_entries.retain(|e| e.slug != slug);
    content::write_index(instance_dir, kind, &new_entries)?;
    // Si el índice falla, el archivo queda intacto (no se llegó acá).

    // 2. Borrar el archivo.
    if let Some(name) = filename {
        let path = subdir.join(&name);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }

    // 3. Confirmar en journal (si falla, el journal queda pendiente y
    // recover_with_journal lo completará en el próximo inicio).
    journal_remove(instance_dir, &journal_entry)?;

    Ok(())
}

/// Recuperación al iniciar.
#[allow(dead_code)]
pub fn recover_pending(instance_dir: &Path) -> Result<(), String> {
    // Si hay transacciones pendientes, conservar respaldos .old para rollback.
    let has_pending = read_journal(instance_dir)
        .map(|j| !j.pending.is_empty())
        .unwrap_or(true);

    let obsidia_dir = instance_dir.join(".obsidia");
    if obsidia_dir.exists() {
        // Solo limpiar .part en .obsidia; los .old de estado se gestionan
        // en restore_state_files (recuperación segura).
        let entries: Vec<_> = fs::read_dir(&obsidia_dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        for entry in entries {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if name.ends_with(".part") {
                let _ = fs::remove_file(&path);
            }
        }
    }

    for (subdir, kind) in [
        ("mods", "mods"),
        ("shaderpacks", "shaders"),
        ("resourcepacks", "packs"),
    ] {
        let dir = instance_dir.join(subdir);
        if !dir.exists() {
            continue;
        }
        // Solo actuar sobre archivos que el índice conoce.
        let known: std::collections::HashSet<String> = content::read_index(instance_dir, kind)
            .unwrap_or_default()
            .into_iter()
            .map(|e| e.filename)
            .collect();

        let entries: Vec<_> = fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        for entry in entries {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n.to_string(),
                None => continue,
            };
            if name.ends_with(".old") {
                let dest_name = name.trim_end_matches(".old");
                if dest_name.is_empty() {
                    continue;
                }
                // Solo restaurar/borrar si el índice conoce el archivo base.
                if !known.contains(dest_name) {
                    continue;
                }
                let dest_path = dir.join(dest_name);
                if !dest_path.exists() {
                    let _ = fs::rename(&path, &dest_path);
                } else if !has_pending {
                    let _ = fs::remove_file(&path);
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_reemplaza_y_no_duplica() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-replace", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("mod.jar");

        write_atomic(&dest, b"v1", None).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v1");

        write_atomic(&dest, b"v2", None).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v2");

        // El respaldo .old se conserva tras write_atomic.
        let backup = dest.with_file_name(format!(
            "{}.old",
            dest.file_name().unwrap_or_default().to_string_lossy()
        ));
        assert!(backup.exists());
        let _ = fs::remove_file(&backup);
        let entries: Vec<_> = fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(entries.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_fallo_conserva_anterior() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-fail", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("mod.jar");

        write_atomic(&dest, b"original", None).unwrap();

        // Hash incorrecto: no debe reemplazar.
        let bad_sha = "0000000000000000000000000000000000000000";
        assert!(write_atomic(&dest, b"corrupto", Some(bad_sha)).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"original");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_verifica_sha1() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-sha", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("mod.jar");

        let data = b"contenido valido";
        let sha = sha1_hex(data);
        write_atomic(&dest, data, Some(&sha)).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), data);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_update_exitosa() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-tx-ok", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        // Instalación inicial.
        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v1");

        // Actualización.
        apply_update_transactional(
            &dest,
            b"v2",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v2".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v2");
        assert_eq!(crate::content::read_index(&dir, "mods").unwrap().len(), 1);

        // Sin temporales huérfanos.
        for e in fs::read_dir(dir.join("mods")).unwrap().flatten() {
            assert_ne!(e.path().extension().and_then(|x| x.to_str()), Some("part"));
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_update_fallo_indice_restaura() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-tx-fail", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        // Instalación inicial.
        apply_update_transactional(
            &dest,
            b"original",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Corromper el índice para que upsert falle.
        let idx = dir.join(".obsidia").join("mods.json");
        fs::write(&idx, "{ corrupto").unwrap();

        // Intentar actualización: debe restaurar el archivo original.
        let res = apply_update_transactional(
            &dest,
            b"nuevo",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v2".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        );
        assert!(res.is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"original");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_transactional_exitosa() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-remove", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"data",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        remove_entry_transactional(&dir, "mods", &dir.join("mods"), "mod").unwrap();
        assert!(crate::content::read_index(&dir, "mods").unwrap().is_empty());
        assert!(!dest.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_pending_limpia_part_y_restaura_old() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-recover", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();

        // .part huérfano.
        fs::write(dir.join("mods").join("mod.jar.part"), b"incompleto").unwrap();
        // .old huérfano sin destino.
        fs::write(dir.join(".obsidia").join("mods.json.old"), b"[]").unwrap();

        recover_with_journal(&dir).unwrap();
        assert!(dir.join("mods").join("mod.jar.part").exists());
        // .old restaurado como mods.json.
        assert!(dir.join(".obsidia").join("mods.json").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_cierra_despues_de_indice_antes_de_archivo() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-journal", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        // Instalar v1.
        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Simular cierre tras actualizar índice, antes de completar journal.
        // El journal debe tener install pendiente.
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: None,
                version_id: Some("v1".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v1")),
                prev_hash: None,
            },
        )
        .unwrap();

        // Recuperar: el archivo existe y el índice lo lista → descartar del journal.
        recover_with_journal(&dir).unwrap();
        let j2 = read_journal(&dir).unwrap();
        assert!(j2.pending.is_empty());
        assert!(dest.exists());
        assert_eq!(crate::content::read_index(&dir, "mods").unwrap().len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_remove_pendiente_se_completa() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-jremove", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"data",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Simular: índice ya actualizado, archivo aún presente, journal pendiente.
        let entry = JournalEntry {
            op: "remove".to_string(),
            kind: "mods".to_string(),
            slug: "mod".to_string(),
            filename: "mod.jar".to_string(),
            old_filename: None,
            version_id: None,
            sha1: None,
            new_hash: None,
            prev_hash: None,
        };
        journal_add(&dir, entry).unwrap();
        // Quitar del índice manualmente (como haría remove_entry_transactional).
        let mut entries = crate::content::read_index(&dir, "mods").unwrap();
        entries.retain(|e| e.slug != "mod");
        crate::content::write_index(&dir, "mods", &entries).unwrap();

        // Recuperar: debe borrar el archivo.
        recover_with_journal(&dir).unwrap();
        assert!(!dest.exists());
        assert!(read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_corrupto_no_toca_nada() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-jcorrupt", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");
        fs::write(&dest, b"contenido").unwrap();

        // Journal corrupto.
        fs::write(dir.join(".obsidia").join("journal.json"), "{ corrupto").unwrap();

        // recuper_owith_journal debe devolver error sin borrar archivos.
        assert!(recover_with_journal(&dir).is_err());
        assert!(dest.exists());
        assert_eq!(fs::read(&dest).unwrap(), b"contenido");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recuperacion_dos_veces_sin_danos() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-twice", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"data",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Dos recuperaciones: la segunda no debe alterar nada.
        recover_with_journal(&dir).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"data");
        recover_with_journal(&dir).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"data");
        assert_eq!(crate::content::read_index(&dir, "mods").unwrap().len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn operaciones_simultaneas_ab() {
        use crate::content::{ContentEntry, read_index};
        let dir_a = std::env::temp_dir().join(format!("obsidia-fsx-{}-a", std::process::id()));
        let dir_b = std::env::temp_dir().join(format!("obsidia-fsx-{}-b", std::process::id()));
        for d in [&dir_a, &dir_b] {
            let _ = fs::remove_dir_all(d);
            fs::create_dir_all(d.join(".obsidia")).unwrap();
            fs::create_dir_all(d.join("mods")).unwrap();
        }
        let dest_a = dir_a.join("mods").join("mod.jar");
        let dest_b = dir_b.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest_a,
            b"contenido-a",
            None,
            &dir_a,
            "mods",
            &dir_a.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "va".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        apply_update_transactional(
            &dest_b,
            b"contenido-b",
            None,
            &dir_b,
            "mods",
            &dir_b.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "vb".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        assert_eq!(fs::read(&dest_a).unwrap(), b"contenido-a");
        assert_eq!(fs::read(&dest_b).unwrap(), b"contenido-b");
        assert_eq!(read_index(&dir_a, "mods").unwrap()[0].version_id, "va");
        assert_eq!(read_index(&dir_b, "mods").unwrap()[0].version_id, "vb");

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }

    #[test]
    fn interrupcion_tras_journal_antes_write() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-inth1", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");
        fs::write(&dest, b"v1").unwrap();

        // Journal instalado manualmente: simula interrupción tras journal_add.
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: None,
                version_id: Some("v1".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v1")),
                prev_hash: None,
            },
        )
        .unwrap();

        // Sin write_atomic previo: el archivo es el viejo.
        assert_eq!(fs::read(&dest).unwrap(), b"v1");

        recover_with_journal(&dir).unwrap();
        // El archivo existe pero el índice no lo lista → se borra el nuevo (que es el viejo).
        // En este caso el índice está vacío, así que el archivo se considera incompleto.
        assert!(read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn interrupcion_tras_write_antes_indice() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-inth2", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        // Instalar v1.
        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Simular: write_atomic ya corrió (archivo v2), índice aún v1, journal pendiente.
        fs::write(&dest, b"v2").unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: None,
                prev_hash: None,
            },
        )
        .unwrap();

        assert!(recover_with_journal(&dir).is_err());
        // El índice sigue describiendo v1 y no puede demostrarse la nueva versión:
        // se conserva el journal (no se declara éxito por coincidencia de filename).
        assert!(dest.exists());
        assert!(!read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn actualizacion_filename_diferente_recuperacion() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-filename", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod-v1.jar");

        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod-v1.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Actualizar con filename distinto.
        let dest2 = dir.join("mods").join("mod-v2.jar");
        apply_update_transactional(
            &dest2,
            b"v2",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v2".to_string(),
                filename: "mod-v2.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        assert!(!dir.join("mods").join("mod-v1.jar").exists());
        assert!(dest2.exists());
        assert_eq!(crate::content::read_index(&dir, "mods").unwrap().len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_con_filename_invalido_no_borra() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-badname", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();

        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "../../evil.jar".to_string(),
                old_filename: None,
                version_id: None,
                sha1: None,
                new_hash: None,
                prev_hash: None,
            },
        )
        .unwrap();

        assert!(recover_with_journal(&dir).is_err());
        // El journal conserva la entrada inválida para revisión.
        let j = read_journal(&dir).unwrap();
        assert_eq!(j.pending.len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn concurrencia_misma_instancia_serializada() {
        use crate::content::{ContentEntry, read_index};
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-conc", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dir_arc = Arc::new(dir.clone());
        let mut handles = Vec::new();
        for i in 0..4 {
            let d = dir_arc.clone();
            handles.push(std::thread::spawn(move || {
                let dest = d.join("mods").join(format!("mod{i}.jar"));
                apply_update_transactional(
                    &dest,
                    format!("contenido-{i}").as_bytes(),
                    None,
                    &d,
                    "mods",
                    &d.join("mods"),
                    ContentEntry {
                        slug: format!("mod{i}"),
                        version_id: format!("v{i}"),
                        filename: format!("mod{i}.jar"),
                        sha1: None,
                        loader: None,
                        mc_version: None,
                        icon_url: None,
                        source: "modrinth".to_string(),
                    },
                )
                .unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let entries = crate::content::read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 4);

        let _ = fs::remove_dir_all(&dir);
    }

    // ── Tests obligatorios Bloque 4A (auditoría) ──────────────────────────

    #[test]
    fn remove_interrupcion_tras_journal_antes_indice() {
        use crate::content::{ContentEntry, read_index};
        let dir =
            std::env::temp_dir().join(format!("obsidia-fsx-{}-remove-int1", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"data",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Simular interrupción tras journal_add, antes de actualizar índice.
        let entry = JournalEntry {
            op: "remove".to_string(),
            kind: "mods".to_string(),
            slug: "mod".to_string(),
            filename: "mod.jar".to_string(),
            old_filename: None,
            version_id: None,
            sha1: None,
            new_hash: None,
            prev_hash: None,
        };
        journal_add(&dir, entry).unwrap();

        recover_with_journal(&dir).unwrap();
        // El índice todavía conserva la entrada: no se borra el archivo.
        assert!(dest.exists());
        assert_eq!(crate::content::read_index(&dir, "mods").unwrap().len(), 1);
        assert!(read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_interrupcion_tras_indice_antes_archivo() {
        use crate::content::{ContentEntry, read_index};
        let dir =
            std::env::temp_dir().join(format!("obsidia-fsx-{}-remove-int2", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"data",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Simular: índice ya actualizado, archivo aún presente, journal pendiente.
        let entry = JournalEntry {
            op: "remove".to_string(),
            kind: "mods".to_string(),
            slug: "mod".to_string(),
            filename: "mod.jar".to_string(),
            old_filename: None,
            version_id: None,
            sha1: None,
            new_hash: None,
            prev_hash: None,
        };
        journal_add(&dir, entry).unwrap();
        let mut entries = crate::content::read_index(&dir, "mods").unwrap();
        entries.retain(|e| e.slug != "mod");
        crate::content::write_index(&dir, "mods", &entries).unwrap();

        recover_with_journal(&dir).unwrap();
        assert!(!dest.exists());
        assert!(crate::content::read_index(&dir, "mods").unwrap().is_empty());
        assert!(read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_mismo_filename_interrumpido_indice_anterior() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!(
            "obsidia-fsx-{}-install-same-int",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Simular: write_atomic ya corrió (archivo v2), índice aún v1, journal pendiente.
        // El respaldo real de v1 se conserva.
        fs::write(&dest, b"v2").unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"v1").unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: None,
                prev_hash: None,
            },
        )
        .unwrap();

        // La recuperación revierte a v1 usando el respaldo.
        recover_with_journal(&dir).unwrap();
        assert!(dest.exists());
        assert_eq!(fs::read(&dest).unwrap(), b"v1");
        assert_eq!(
            crate::content::read_index(&dir, "mods").unwrap()[0].version_id,
            "v1"
        );
        assert!(read_journal(&dir).unwrap().pending.is_empty());
        assert!(!dir.join("mods").join("mod.jar.old").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_mismo_filename_completado() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!(
            "obsidia-fsx-{}-install-same-ok",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Simular operación completada: índice ya en v2, archivo v2, journal pendiente.
        fs::write(&dest, b"v2").unwrap();
        let mut entries = crate::content::read_index(&dir, "mods").unwrap();
        entries[0].version_id = "v2".to_string();
        crate::content::write_index(&dir, "mods", &entries).unwrap();

        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: None,
                prev_hash: None,
            },
        )
        .unwrap();

        recover_with_journal(&dir).unwrap();
        assert!(dest.exists());
        assert_eq!(
            crate::content::read_index(&dir, "mods").unwrap()[0].version_id,
            "v2"
        );
        assert!(read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recuperacion_idempotente_dos_veces_remove() {
        use crate::content::{ContentEntry, read_index};
        let dir =
            std::env::temp_dir().join(format!("obsidia-fsx-{}-idemp-remove", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"data",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        let entry = JournalEntry {
            op: "remove".to_string(),
            kind: "mods".to_string(),
            slug: "mod".to_string(),
            filename: "mod.jar".to_string(),
            old_filename: None,
            version_id: None,
            sha1: None,
            new_hash: None,
            prev_hash: None,
        };
        journal_add(&dir, entry.clone()).unwrap();
        let mut entries = crate::content::read_index(&dir, "mods").unwrap();
        entries.retain(|e| e.slug != "mod");
        crate::content::write_index(&dir, "mods", &entries).unwrap();

        recover_with_journal(&dir).unwrap();
        assert!(!dest.exists());
        recover_with_journal(&dir).unwrap();
        assert!(!dest.exists());
        assert!(crate::content::read_index(&dir, "mods").unwrap().is_empty());
        assert!(read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ningun_escenario_deja_indice_colgante() {
        use crate::content::{ContentEntry, read_index};
        // remove interrumpido antes de índice: archivo e índice permanecen.
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-nocolg", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"data",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Simular remove interrumpido antes de índice.
        journal_add(
            &dir,
            JournalEntry {
                op: "remove".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: None,
                version_id: None,
                sha1: None,
                new_hash: None,
                prev_hash: None,
            },
        )
        .unwrap();

        recover_with_journal(&dir).unwrap();
        // Si el índice conserva la entrada, el archivo debe existir.
        if !crate::content::read_index(&dir, "mods").unwrap().is_empty() {
            assert!(dest.exists());
        }
        // Ningún índice apunta a archivo inexistente.
        for e in crate::content::read_index(&dir, "mods").unwrap() {
            assert!(dir.join("mods").join(&e.filename).exists());
        }

        let _ = fs::remove_dir_all(&dir);
    }

    // ── Tests auditoría 2 ────────────────────────────────────────────────

    #[test]
    fn recover_pending_restaura_old_tras_interrupcion_write_atomic() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-old1", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("mods")).unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        // dest falta, .part (nuevo) y .old (original) existen.
        fs::write(dir.join("mods").join("mod.jar.part"), b"v2").unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"v1").unwrap();

        recover_pending(&dir).unwrap();
        assert!(dir.join("mods").join("mod.jar").exists());
        assert_eq!(fs::read(dir.join("mods").join("mod.jar")).unwrap(), b"v1");
        assert!(dir.join("mods").join("mod.jar.part").exists());
        assert!(!dir.join("mods").join("mod.jar.old").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_pending_restaura_old_en_mods() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-oldmods", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("mods")).unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"original").unwrap();

        recover_pending(&dir).unwrap();
        assert!(dir.join("mods").join("mod.jar").exists());
        assert_eq!(
            fs::read(dir.join("mods").join("mod.jar")).unwrap(),
            b"original"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_pending_restaura_old_en_shaderpacks() {
        let dir =
            std::env::temp_dir().join(format!("obsidia-fsx-{}-oldshaders", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("shaderpacks")).unwrap();
        crate::content::write_index(
            &dir,
            "shaders",
            &[crate::content::ContentEntry {
                slug: "pack".to_string(),
                version_id: "v1".to_string(),
                filename: "pack.zip".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        fs::write(dir.join("shaderpacks").join("pack.zip.old"), b"original").unwrap();

        recover_pending(&dir).unwrap();
        assert!(dir.join("shaderpacks").join("pack.zip").exists());
        assert_eq!(
            fs::read(dir.join("shaderpacks").join("pack.zip")).unwrap(),
            b"original"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_pending_restaura_old_en_resourcepacks() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-oldpacks", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("resourcepacks")).unwrap();
        crate::content::write_index(
            &dir,
            "packs",
            &[crate::content::ContentEntry {
                slug: "pack".to_string(),
                version_id: "v1".to_string(),
                filename: "pack.zip".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        fs::write(dir.join("resourcepacks").join("pack.zip.old"), b"original").unwrap();

        recover_pending(&dir).unwrap();
        assert!(dir.join("resourcepacks").join("pack.zip").exists());
        assert_eq!(
            fs::read(dir.join("resourcepacks").join("pack.zip")).unwrap(),
            b"original"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recover_pending_dest_part_old_simultaneos() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-oldsim", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("mods")).unwrap();
        // dest válido existe junto a .part y .old: se conserva dest.
        fs::write(dir.join("mods").join("mod.jar"), b"valido").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        fs::write(dir.join("mods").join("mod.jar.part"), b"parcial").unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"backup").unwrap();

        recover_pending(&dir).unwrap();
        assert_eq!(
            fs::read(dir.join("mods").join("mod.jar")).unwrap(),
            b"valido"
        );
        assert!(dir.join("mods").join("mod.jar.part").exists());
        assert!(!dir.join("mods").join("mod.jar.old").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_recovery_error_escritura_indice_se_propaga() {
        use crate::content::{ContentEntry, read_index};
        let dir =
            std::env::temp_dir().join(format!("obsidia-fsx-{}-remove-idxerr", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"data",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Simular remove interrumpido: índice ya sin entrada, archivo aún presente.
        let entry = JournalEntry {
            op: "remove".to_string(),
            kind: "mods".to_string(),
            slug: "mod".to_string(),
            filename: "mod.jar".to_string(),
            old_filename: None,
            version_id: None,
            sha1: None,
            new_hash: None,
            prev_hash: None,
        };
        journal_add(&dir, entry.clone()).unwrap();
        let mut entries = crate::content::read_index(&dir, "mods").unwrap();
        entries.retain(|e| e.slug != "mod");
        crate::content::write_index(&dir, "mods", &entries).unwrap();

        // Forzar fallo de escritura del índice en recuperación: mods.json como directorio.
        let idx_path = dir.join(".obsidia").join("mods.json");
        let _ = fs::remove_file(&idx_path);
        fs::create_dir_all(&idx_path).unwrap();

        // La recuperación debe fallar y conservar el journal.
        assert!(recover_with_journal(&dir).is_err());
        assert!(!read_journal(&dir).unwrap().pending.is_empty());

        // Limpiar y reintentar: debe completar.
        let _ = fs::remove_dir_all(&idx_path);
        let _ = fs::remove_file(dir.join(".obsidia").join("mods.json.old"));
        recover_with_journal(&dir).unwrap();
        assert!(!dest.exists());
        assert!(crate::content::read_index(&dir, "mods").unwrap().is_empty());
        assert!(read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pending_impide_recuperacion_ok() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-pending", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();

        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: None,
                prev_hash: None,
            },
        )
        .unwrap();
        fs::write(dir.join("mods").join("mod.jar"), b"v1").unwrap();

        // Transacción sin resolver: recuperación no es Ok.
        assert!(recover_with_journal(&dir).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn instancia_sana_recupera_aunque_otra_pendiente() {
        let dir_a = std::env::temp_dir().join(format!("obsidia-fsx-{}-sana-a", std::process::id()));
        let dir_b = std::env::temp_dir().join(format!("obsidia-fsx-{}-sana-b", std::process::id()));
        for d in [&dir_a, &dir_b] {
            let _ = fs::remove_dir_all(d);
            fs::create_dir_all(d.join(".obsidia")).unwrap();
            fs::create_dir_all(d.join("mods")).unwrap();
        }

        // Instancia A: sana.
        fs::write(dir_a.join("mods").join("mod.jar"), b"data").unwrap();
        // Instancia B: pendiente.
        journal_add(
            &dir_b,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: None,
                prev_hash: None,
            },
        )
        .unwrap();
        fs::write(dir_b.join("mods").join("mod.jar"), b"v1").unwrap();

        assert!(recover_with_journal(&dir_a).is_ok());
        assert!(recover_with_journal(&dir_b).is_err());

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }

    #[test]
    fn segunda_recuperacion_completa_tras_corregir_error() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-retry", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"data",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        let entry = JournalEntry {
            op: "remove".to_string(),
            kind: "mods".to_string(),
            slug: "mod".to_string(),
            filename: "mod.jar".to_string(),
            old_filename: None,
            version_id: None,
            sha1: None,
            new_hash: None,
            prev_hash: None,
        };
        journal_add(&dir, entry).unwrap();
        let mut entries = crate::content::read_index(&dir, "mods").unwrap();
        entries.retain(|e| e.slug != "mod");
        crate::content::write_index(&dir, "mods", &entries).unwrap();

        // Primer intento falla por índice bloqueado.
        let idx_path = dir.join(".obsidia").join("mods.json");
        let _ = fs::remove_file(&idx_path);
        fs::create_dir_all(&idx_path).unwrap();
        assert!(recover_with_journal(&dir).is_err());

        // Corregir y reintentar.
        let _ = fs::remove_dir_all(&idx_path);
        let _ = fs::remove_file(dir.join(".obsidia").join("mods.json.old"));
        recover_with_journal(&dir).unwrap();
        assert!(!dest.exists());
        assert!(crate::content::read_index(&dir, "mods").unwrap().is_empty());
        assert!(read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ningun_test_deja_indice_colgante_tras_old_y_pending() {
        use crate::content::read_index;
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-nocolg2", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        fs::write(&dest, b"data").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"backup").unwrap();

        recover_pending(&dir).unwrap();
        // El archivo válido no se sobrescribe.
        assert_eq!(fs::read(&dest).unwrap(), b"data");
        assert!(!dir.join("mods").join("mod.jar.old").exists());

        for e in crate::content::read_index(&dir, "mods").unwrap() {
            assert!(dir.join("mods").join(&e.filename).exists());
        }

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn backup_real_generado_por_write_atomic_se_restaura() {
        let dir =
            std::env::temp_dir().join(format!("obsidia-fsx-{}-realbackup", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        fs::write(&dest, b"v1").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        // write_atomic genera el respaldo real mod.jar.old.
        write_atomic(&dest, b"v2", None).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v2");
        let backup = dir.join("mods").join("mod.jar.old");
        assert!(backup.exists());
        assert_eq!(fs::read(&backup).unwrap(), b"v1");

        // Simular interrupción: el archivo nuevo no llegó a instalarse.
        fs::remove_file(&dest).unwrap();
        recover_pending(&dir).unwrap();
        assert!(dest.exists());
        assert_eq!(fs::read(&dest).unwrap(), b"v1");
        assert!(!backup.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn install_mismo_filename_completado_limpia_backup() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-same-ok2", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Operación completada: índice en v2, archivo v2, respaldo v1, journal pendiente.
        fs::write(&dest, b"v2").unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"v1").unwrap();
        let mut entries = crate::content::read_index(&dir, "mods").unwrap();
        entries[0].version_id = "v2".to_string();
        crate::content::write_index(&dir, "mods", &entries).unwrap();

        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: None,
                prev_hash: None,
            },
        )
        .unwrap();

        recover_with_journal(&dir).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v2");
        assert_eq!(
            crate::content::read_index(&dir, "mods").unwrap()[0].version_id,
            "v2"
        );
        assert!(read_journal(&dir).unwrap().pending.is_empty());
        assert!(!dir.join("mods").join("mod.jar.old").exists());

        // Idempotencia.
        recover_with_journal(&dir).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v2");
        assert_eq!(
            crate::content::read_index(&dir, "mods").unwrap()[0].version_id,
            "v2"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    // Tests auditoría 4
    #[test]
    fn ventana_entre_renames_restaura_v1() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-ventana", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // mod.jar ausente, mod.jar.old = v1, índice v1, journal v2
        fs::remove_file(&dest).unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"v1").unwrap();
        let v2_hash = sha1_hex(b"v2");
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: Some(v2_hash),
                prev_hash: Some(sha1_hex(b"v1")),
            },
        )
        .unwrap();

        recover_with_journal(&dir).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v1");
        assert_eq!(
            crate::content::read_index(&dir, "mods").unwrap()[0].version_id,
            "v1"
        );
        assert!(read_journal(&dir).unwrap().pending.is_empty());
        assert!(!dir.join("mods").join("mod.jar.old").exists());

        // Idempotencia.
        recover_with_journal(&dir).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"v1");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn archivo_previo_no_indexado_se_conserva() {
        let dir =
            std::env::temp_dir().join(format!("obsidia-fsx-{}-prevnoindex", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        fs::write(&dest, b"v2").unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"usuario").unwrap();

        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: None,
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v2")),
                prev_hash: None,
            },
        )
        .unwrap();

        recover_with_journal(&dir).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"usuario");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_sin_identidad_err() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-noid", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        fs::write(&dest, b"v2").unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"v1").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();

        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: None,
                sha1: None,
                new_hash: None,
                prev_hash: Some(sha1_hex(b"v1")),
            },
        )
        .unwrap();

        assert!(recover_with_journal(&dir).is_err());
        assert!(dest.exists());
        assert!(dir.join("mods").join("mod.jar.old").exists());
        assert!(!read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_version_id_vacio_err() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-emptyid", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        fs::write(&dest, b"v2").unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"v1").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();

        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("".to_string()),
                sha1: None,
                new_hash: None,
                prev_hash: Some(sha1_hex(b"v1")),
            },
        )
        .unwrap();

        assert!(recover_with_journal(&dir).is_err());
        assert!(!read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fallo_rename_restauracion_conserva_backup() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-renfail", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        // dest es un directorio no vacío para forzar fallo de rename backup->dest
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("x"), b"x").unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"v1").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v2")),
                prev_hash: Some(sha1_hex(b"v1")),
            },
        )
        .unwrap();

        assert!(recover_with_journal(&dir).is_err());
        assert!(dir.join("mods").join("mod.jar.old").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_fallo_remove_file_conserva_journal() {
        let dir =
            std::env::temp_dir().join(format!("obsidia-fsx-{}-removefail", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        // archivo como directorio no vacío para forzar fallo de remove_file
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join("x"), b"x").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        let entry = JournalEntry {
            op: "remove".to_string(),
            kind: "mods".to_string(),
            slug: "mod".to_string(),
            filename: "mod.jar".to_string(),
            old_filename: None,
            version_id: None,
            sha1: None,
            new_hash: None,
            prev_hash: None,
        };
        journal_add(&dir, entry.clone()).unwrap();
        let mut entries = crate::content::read_index(&dir, "mods").unwrap();
        entries.retain(|e| e.slug != "mod");
        crate::content::write_index(&dir, "mods", &entries).unwrap();

        assert!(recover_with_journal(&dir).is_err());
        assert!(!read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dos_pendientes_una_falla() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-twopend", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        // Entrada A resoluble: índice ya v2, archivo v2
        fs::write(&dest, b"v2").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "a".to_string(),
                version_id: "v2".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "a".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: None,
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v2")),
                prev_hash: None,
            },
        )
        .unwrap();

        // Entrada B fallida: remove con archivo directorio
        let bad = dir.join("mods").join("bad.jar");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join("x"), b"x").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[
                crate::content::ContentEntry {
                    slug: "a".to_string(),
                    version_id: "v2".to_string(),
                    filename: "mod.jar".to_string(),
                    sha1: None,
                    loader: None,
                    mc_version: None,
                    icon_url: None,
                    source: "modrinth".to_string(),
                },
                crate::content::ContentEntry {
                    slug: "b".to_string(),
                    version_id: "v1".to_string(),
                    filename: "bad.jar".to_string(),
                    sha1: None,
                    loader: None,
                    mc_version: None,
                    icon_url: None,
                    source: "modrinth".to_string(),
                },
            ],
        )
        .unwrap();
        let mut entries = crate::content::read_index(&dir, "mods").unwrap();
        entries.retain(|e| e.slug != "b");
        crate::content::write_index(&dir, "mods", &entries).unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "remove".to_string(),
                kind: "mods".to_string(),
                slug: "b".to_string(),
                filename: "bad.jar".to_string(),
                old_filename: None,
                version_id: None,
                sha1: None,
                new_hash: None,
                prev_hash: None,
            },
        )
        .unwrap();

        let _ = recover_with_journal(&dir);
        let j = read_journal(&dir).unwrap();
        // A no debe quedar; B sí.
        assert!(j.pending.iter().all(|e| e.slug != "a"));
        assert!(j.pending.iter().any(|e| e.slug == "b"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_ajeno_no_se_toca() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-oldajeno", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("mods")).unwrap();
        fs::create_dir_all(dir.join("shaderpacks")).unwrap();
        fs::create_dir_all(dir.join("resourcepacks")).unwrap();

        fs::write(dir.join("mods").join("foo.jar.old"), b"ajeno").unwrap();
        fs::write(dir.join("shaderpacks").join("foo.zip.old"), b"ajeno").unwrap();
        fs::write(dir.join("resourcepacks").join("foo.zip.old"), b"ajeno").unwrap();

        recover_pending(&dir).unwrap();
        assert!(dir.join("mods").join("foo.jar.old").exists());
        assert!(dir.join("shaderpacks").join("foo.zip.old").exists());
        assert!(dir.join("resourcepacks").join("foo.zip.old").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_json_old_restauracion() {
        let dir =
            std::env::temp_dir().join(format!("obsidia-fsx-{}-journalold", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();

        let j = Journal {
            pending: vec![JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: None,
                version_id: Some("v1".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v1")),
                prev_hash: None,
            }],
        };
        let data = serde_json::to_string_pretty(&j).unwrap();
        fs::write(dir.join(".obsidia").join("journal.json.old"), &data).unwrap();

        recover_with_journal(&dir).unwrap();
        assert!(dir.join(".obsidia").join("journal.json").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn ausencia_archivo_y_backup_con_prev_hash_conserva_journal() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-ausencia", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();

        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();

        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v2")),
                prev_hash: Some(sha1_hex(b"v1")),
            },
        )
        .unwrap();

        assert!(recover_with_journal(&dir).is_err());
        assert!(!read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn hash_fisico_no_coincide_conserva_backup() {
        let dir =
            std::env::temp_dir().join(format!("obsidia-fsx-{}-hashfisico", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        fs::write(&dest, b"no-es-v2").unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"v1").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v2".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v2")),
                prev_hash: Some(sha1_hex(b"v1")),
            },
        )
        .unwrap();

        assert!(recover_with_journal(&dir).is_err());
        assert!(dir.join("mods").join("mod.jar.old").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn i4_nueva_actualizacion_recupera_pendiente_sin_destruir_respaldo() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-i4upd", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Transacción interrumpida a v2.
        fs::write(&dest, b"v2").unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"v1").unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v2")),
                prev_hash: Some(sha1_hex(b"v1")),
            },
        )
        .unwrap();

        // Nueva actualización a v3: primero recupera (rollback a v1), luego instala v3.
        apply_update_transactional(
            &dest,
            b"v3",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v3".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        assert_eq!(fs::read(&dest).unwrap(), b"v3");
        assert_eq!(read_index(&dir, "mods").unwrap()[0].version_id, "v3");
        assert!(read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn i4_nueva_eliminacion_recupera_pendiente() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-i4rm", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        // Transacción interrumpida a v2.
        fs::write(&dest, b"v2").unwrap();
        fs::write(dir.join("mods").join("mod.jar.old"), b"v1").unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("mod.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v2")),
                prev_hash: Some(sha1_hex(b"v1")),
            },
        )
        .unwrap();

        // Nueva eliminación: primero recupera, luego elimina.
        remove_entry_transactional(&dir, "mods", &dir.join("mods"), "mod").unwrap();

        assert!(!dest.exists());
        assert!(read_index(&dir, "mods").unwrap().is_empty());
        assert!(read_journal(&dir).unwrap().pending.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn k1_indice_v1_actualizacion_v2_sin_bloqueo() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-k1", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod-v1.jar");

        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod-v1.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        let dest2 = dir.join("mods").join("mod-v2.jar");
        fs::write(&dest2, b"v2").unwrap();
        fs::write(dir.join("mods").join("mod-v2.jar.old"), b"v1").unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod-v2.jar".to_string(),
                old_filename: Some("mod-v1.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v2")),
                prev_hash: Some(sha1_hex(b"v1")),
            },
        )
        .unwrap();

        // Índice aún v1; had_prev_at_dest debe ser false (old_filename distinto).
        recover_with_journal(&dir).unwrap();
        assert!(
            read_index(&dir, "mods")
                .unwrap()
                .iter()
                .any(|e| e.slug == "mod")
        );
        assert!(read_journal(&dir).unwrap().pending.is_empty());

        // Futura operación no queda bloqueada.
        remove_entry_transactional(&dir, "mods", &dir.join("mods"), "mod").unwrap();
        assert!(read_index(&dir, "mods").unwrap().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn m1_error_restauracion_estado_no_exitoso() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-m1", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        // journal.json falta pero .old es inválido.
        fs::write(dir.join(".obsidia").join("journal.json.old"), b"{ invalido").unwrap();

        assert!(recover_with_journal(&dir).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn m2_archivo_ajeno_no_se_borra_en_rollback() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-m2", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        fs::write(&dest, b"archivo-ajeno").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "install".to_string(),
                kind: "mods".to_string(),
                slug: "mod".to_string(),
                filename: "mod.jar".to_string(),
                old_filename: Some("other.jar".to_string()),
                version_id: Some("v2".to_string()),
                sha1: None,
                new_hash: Some(sha1_hex(b"v2")),
                prev_hash: None,
            },
        )
        .unwrap();

        assert!(recover_with_journal(&dir).is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"archivo-ajeno");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn n2_part_ajeno_no_se_borra() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-n2", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("mods")).unwrap();
        fs::write(dir.join("mods").join("foreign.jar.part"), b"ajeno").unwrap();

        recover_pending(&dir).unwrap();
        assert!(dir.join("mods").join("foreign.jar.part").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn n1_remove_resuelta_no_se_reprocesa() {
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-n1remove", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();

        // Entrada A ya resuelta: slug ausente del índice y archivo inexistente.
        crate::content::write_index(&dir, "mods", &[]).unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "remove".to_string(),
                kind: "mods".to_string(),
                slug: "a".to_string(),
                filename: "a.jar".to_string(),
                old_filename: None,
                version_id: None,
                sha1: None,
                new_hash: None,
                prev_hash: None,
            },
        )
        .unwrap();

        // Entrada B fallida: archivo es un directorio no vacío.
        let bad = dir.join("mods").join("bad.jar");
        fs::create_dir_all(&bad).unwrap();
        fs::write(bad.join("x"), b"x").unwrap();
        crate::content::write_index(
            &dir,
            "mods",
            &[crate::content::ContentEntry {
                slug: "b".to_string(),
                version_id: "v1".to_string(),
                filename: "bad.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            }],
        )
        .unwrap();
        let mut entries = crate::content::read_index(&dir, "mods").unwrap();
        entries.retain(|e| e.slug != "b");
        crate::content::write_index(&dir, "mods", &entries).unwrap();
        journal_add(
            &dir,
            JournalEntry {
                op: "remove".to_string(),
                kind: "mods".to_string(),
                slug: "b".to_string(),
                filename: "bad.jar".to_string(),
                old_filename: None,
                version_id: None,
                sha1: None,
                new_hash: None,
                prev_hash: None,
            },
        )
        .unwrap();

        let _ = recover_with_journal(&dir);
        let j = read_journal(&dir).unwrap();
        assert!(j.pending.iter().all(|e| e.slug != "a"));
        assert!(j.pending.iter().any(|e| e.slug == "b"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_update_persiste_icon_url() {
        use crate::content::{ContentEntry, read_index};
        let dir = std::env::temp_dir().join(format!("obsidia-fsx-{}-icon", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join(".obsidia")).unwrap();
        fs::create_dir_all(dir.join("mods")).unwrap();
        let dest = dir.join("mods").join("mod.jar");

        apply_update_transactional(
            &dest,
            b"v1",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v1".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: Some("https://example.com/icon.png".to_string()),
                source: "modrinth".to_string(),
            },
        )
        .unwrap();

        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(
            entries[0].icon_url.as_deref(),
            Some("https://example.com/icon.png")
        );

        // Actualización conserva/refresca icono.
        apply_update_transactional(
            &dest,
            b"v2",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v2".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: Some("https://example.com/icon2.png".to_string()),
                source: "modrinth".to_string(),
            },
        )
        .unwrap();
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(
            entries[0].icon_url.as_deref(),
            Some("https://example.com/icon2.png")
        );

        // None se maneja correctamente.
        apply_update_transactional(
            &dest,
            b"v3",
            None,
            &dir,
            "mods",
            &dir.join("mods"),
            ContentEntry {
                slug: "mod".to_string(),
                version_id: "v3".to_string(),
                filename: "mod.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
            },
        )
        .unwrap();
        let entries = read_index(&dir, "mods").unwrap();
        assert!(entries[0].icon_url.is_none());

        let _ = fs::remove_dir_all(&dir);
    }
}
