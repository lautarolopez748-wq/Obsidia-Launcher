//! Instancias identificadas por `id` y marcador `.obsidia/instance.json`.

use crate::paths::{InstanceRef, Paths};
use crate::{Profile, read_json_list, write_json_list};
use serde::{Deserialize, Serialize};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Clone)]
pub struct InstanceMarker {
    pub id: String,
    pub name: String,
    pub schema: u32,
    pub origin: Option<String>,
}

const ADMITTED_SCHEMA: u32 = 2;

fn validate_profile(profile: &Profile) -> Result<(), String> {
    if profile.name.trim().is_empty() {
        return Err("El nombre del perfil no puede estar vacío".to_string());
    }
    if let Some(id) = &profile.id {
        crate::paths::InstanceId::parse(id)?;
    }
    Ok(())
}

fn now_string() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

pub fn ensure_marker(paths: &Paths, profile: &Profile) -> Result<(), String> {
    validate_profile(profile)?;
    let Some(id) = &profile.id else {
        return Ok(());
    };
    let inst = match InstanceRef::parse(id)? {
        InstanceRef::Instance(x) => x,
        InstanceRef::Legacy => {
            return Err("legacy no es un id de instancia válido".to_string());
        }
    };
    let dir = paths.instance_dir_for(&inst)?;
    fs::create_dir_all(dir.join(".obsidia")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let marker = InstanceMarker {
        id: id.clone(),
        name: profile.name.clone(),
        schema: profile.schema,
        origin: profile.origin.clone(),
    };
    let data = serde_json::to_string_pretty(&marker).map_err(|e| e.to_string())?;
    fs::write(dir.join(".obsidia").join("instance.json"), data)
        .map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(())
}

fn read_marker(dir: &std::path::Path) -> Result<InstanceMarker, String> {
    let marker_path = dir.join(".obsidia").join("instance.json");
    let data =
        fs::read_to_string(&marker_path).map_err(|e| format!("{}: {e}", marker_path.display()))?;
    let marker: InstanceMarker =
        serde_json::from_str(&data).map_err(|e| format!("{}: {e}", marker_path.display()))?;
    Ok(marker)
}

fn validate_marker(marker: &InstanceMarker, expected_id: &str) -> Result<(), String> {
    if marker.id != expected_id {
        return Err(format!(
            "El marcador pertenece a {} y no a {}",
            marker.id, expected_id
        ));
    }
    if marker.schema != ADMITTED_SCHEMA {
        return Err(format!(
            "Esquema de marcador no admitido: {}",
            marker.schema
        ));
    }
    if marker.name.trim().is_empty() {
        return Err("El marcador no tiene nombre válido".to_string());
    }
    Ok(())
}

fn prepare_trash(paths: &Paths) -> Result<std::path::PathBuf, String> {
    paths.validate_instances_root()?;
    let trash = paths.instances_root.join(".trash");
    if trash.exists() {
        let meta = fs::symlink_metadata(&trash).map_err(|e| format!("{}: {e}", trash.display()))?;
        if crate::paths::is_reparse_meta(&meta) {
            return Err(format!(
                "{} no puede ser un junction ni un symlink",
                trash.display()
            ));
        }
        if !meta.is_dir() {
            return Err(format!("{} no es una carpeta", trash.display()));
        }
    } else {
        fs::create_dir_all(&trash).map_err(|e| format!("{}: {e}", trash.display()))?;
    }
    if crate::paths::is_reparse_point(&trash) {
        return Err(format!(
            "{} no puede ser un junction ni un symlink",
            trash.display()
        ));
    }
    Ok(trash)
}

fn validate_saves_for_deletion(paths: &Paths, dir: &std::path::Path) -> Result<(), String> {
    let saves = dir.join("saves");
    if !saves.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(&saves).map_err(|e| format!("{}: {e}", saves.display()))?;
    if crate::paths::is_reparse_meta(&meta) {
        let real = fs::canonicalize(&saves).map_err(|e| format!("{}: {e}", saves.display()))?;
        let shared = fs::canonicalize(paths.shared_saves())
            .map_err(|e| format!("{}: {e}", paths.shared_saves().display()))?;
        if !crate::paths::same_path(&real, &shared) {
            return Err(format!(
                "{} apunta a {} en vez de {}",
                saves.display(),
                real.display(),
                shared.display()
            ));
        }
    } else {
        // Toda carpeta real, vacía o con contenido, requiere resolución manual.
        return Err(format!(
            "{} es una carpeta saves local; requiere resolución manual",
            saves.display()
        ));
    }
    Ok(())
}

pub fn create_instance(paths: &Paths, mut profile: Profile) -> Result<(), String> {
    validate_profile(&profile)?;
    let path = paths.profiles_file();
    let mut profiles = read_json_list::<Profile>(&path)?;
    if let Some(id) = &profile.id
        && profiles
            .iter()
            .any(|p| p.id.as_deref() == Some(id.as_str()))
    {
        return Err(format!("Ya existe una instancia con id {}", id));
    }
    if profiles.iter().any(|p| p.name == profile.name) {
        return Err(format!("Ya existe un perfil con nombre {}", profile.name));
    }

    profile.origin = Some("native".to_string());
    if profile.created_at.is_none() {
        profile.created_at = Some(now_string());
    }

    if let Some(id) = &profile.id {
        let inst = match InstanceRef::parse(id)? {
            InstanceRef::Instance(x) => x,
            InstanceRef::Legacy => {
                return Err("legacy no es un id de instancia válido".to_string());
            }
        };
        let dir = paths.instance_dir_for(&inst)?;
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        paths.ensure_saves_link(&dir)?;
    }

    profiles.push(profile.clone());
    write_json_list(&path, &profiles)?;
    ensure_marker(paths, &profile)?;
    Ok(())
}

pub fn update_instance(paths: &Paths, id: String, mut profile: Profile) -> Result<(), String> {
    validate_profile(&profile)?;
    let path = paths.profiles_file();
    let mut profiles = read_json_list::<Profile>(&path)?;
    let pos = profiles
        .iter()
        .position(|p| p.id.as_deref() == Some(id.as_str()))
        .ok_or_else(|| format!("No existe una instancia con id {}", id))?;

    let existing = profiles[pos].clone();
    if profile.id.is_none() {
        profile.id = Some(id.clone());
    } else if profile.id.as_deref() != Some(id.as_str()) {
        return Err("El id del perfil no coincide con el solicitado".to_string());
    }

    if profiles
        .iter()
        .enumerate()
        .any(|(i, p)| i != pos && p.name == profile.name)
    {
        return Err(format!("Ya existe un perfil con nombre {}", profile.name));
    }

    if profile.created_at.is_none() {
        profile.created_at = existing.created_at.clone();
    }
    if existing.origin.is_some() {
        profile.origin = existing.origin.clone();
    } else if profile.origin.is_none() {
        profile.origin = Some("native".to_string());
    }

    profiles[pos] = profile.clone();
    write_json_list(&path, &profiles)?;
    ensure_marker(paths, &profile)?;
    Ok(())
}

pub fn delete_instance(paths: &Paths, id: String) -> Result<(), String> {
    paths.validate_instances_root()?;
    let inst = match InstanceRef::parse(&id)? {
        InstanceRef::Instance(x) => x,
        InstanceRef::Legacy => {
            return Err("legacy no es un id de instancia válido".to_string());
        }
    };
    let dir = paths.instance_dir_for(&inst)?;

    if dir.exists() {
        let marker = read_marker(&dir)?;
        validate_marker(&marker, &id)?;
        validate_saves_for_deletion(paths, &dir)?;
    }

    let path = paths.profiles_file();
    let profiles = read_json_list::<Profile>(&path)?;
    let matching: Vec<usize> = profiles
        .iter()
        .enumerate()
        .filter(|(_, p)| p.id.as_deref() == Some(id.as_str()))
        .map(|(i, _)| i)
        .collect();
    if matching.len() > 1 {
        return Err(format!(
            "La instancia {} sigue asociada a {} perfiles; no se elimina",
            id,
            matching.len()
        ));
    }
    let pos = matching
        .first()
        .copied()
        .ok_or_else(|| format!("No existe una instancia con id {}", id))?;

    let trash = if dir.exists() {
        Some(prepare_trash(paths)?)
    } else {
        None
    };
    let dest = trash.as_ref().map(|t| {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        t.join(format!("{}-{}", id, ts))
    });
    if let Some(dest) = &dest
        && dest.exists()
    {
        return Err(format!(
            "Ya existe un destino en la papelera: {}",
            dest.display()
        ));
    }

    if dir.exists() {
        let dest = dest.expect("destino de papelera");
        fs::rename(&dir, &dest).map_err(|e| format!("{}: {e}", dir.display()))?;

        let mut profiles = profiles;
        if pos < profiles.len() {
            profiles.remove(pos);
        }
        if let Err(e) = write_json_list(&path, &profiles) {
            match fs::rename(&dest, &dir) {
                Ok(()) => {
                    return Err(format!(
                        "No se pudo actualizar profiles.json: {e}. La carpeta se restauró a {}.",
                        dir.display()
                    ));
                }
                Err(re) => {
                    return Err(format!(
                        "No se pudo actualizar profiles.json: {e}. Además falló la restauración: {} -> {}: {re}",
                        dest.display(),
                        dir.display()
                    ));
                }
            }
        }
    } else {
        let mut profiles = read_json_list::<Profile>(&path)?;
        if pos < profiles.len() {
            profiles.remove(pos);
        }
        write_json_list(&path, &profiles)?;
    }

    Ok(())
}
