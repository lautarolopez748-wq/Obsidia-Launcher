//! Orden real de los resource packs.
//!
//! Minecraft guarda los packs activos en `options.txt` (`resourcePacks:[...]`).
//! El orden de esa lista es la prioridad: el primero gana. Este módulo lee ese
//! estado y lo reescribe cuando la UI cambia el orden o activa/desactiva un pack.

use crate::skin;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const KEY: &str = "resourcePacks:";
const PREFIX: &str = "file/";

#[derive(Serialize, Clone, PartialEq, Debug)]
pub struct PackState {
    pub name: String,
    pub enabled: bool,
}

fn packs_dir(mc: &Path) -> PathBuf {
    mc.join("resourcepacks")
}

fn options_path(mc: &Path) -> PathBuf {
    mc.join("options.txt")
}

/// Archivos y carpetas de `resourcepacks/`, ordenados.
pub fn list_names(mc: &Path) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(packs_dir(mc)) {
        for e in entries.flatten() {
            let p = e.path();
            if (p.is_file() || p.is_dir())
                && let Some(n) = e.file_name().to_str() {
                    names.push(n.to_string());
                }
        }
    }
    names.sort();
    names
}

/// Entradas activas tal como están en `options.txt`, sin el prefijo `file/`.
pub fn read_active(mc: &Path) -> Vec<String> {
    let Ok(content) = fs::read_to_string(options_path(mc)) else {
        return Vec::new();
    };
    let line = content
        .split('\n')
        .map(|l| l.trim_end_matches('\r'))
        .find(|l| l.starts_with(KEY));
    let Some(line) = line else { return Vec::new() };
    skin::parse_pack_list(line)
        .into_iter()
        .filter_map(|e| e.strip_prefix(PREFIX).map(String::from))
        .collect()
}

/// Estado para la UI: primero los activos (en su orden de prioridad) y después el resto.
pub fn merge_state(names: Vec<String>, active: &[String]) -> Vec<PackState> {
    let available: HashSet<&str> = names.iter().map(String::as_str).collect();
    let mut out: Vec<PackState> = Vec::new();
    for name in active {
        if available.contains(name.as_str()) && !out.iter().any(|p| &p.name == name) {
            out.push(PackState { name: name.clone(), enabled: true });
        }
    }
    for name in names {
        if !out.iter().any(|p| p.name == name) {
            out.push(PackState { name, enabled: false });
        }
    }
    out
}

pub fn get_state(mc: &Path) -> Vec<PackState> {
    merge_state(list_names(mc), &read_active(mc))
}

/// Guarda el orden recibido. Solo acepta packs que existan en `resourcepacks/`.
pub fn save_state(mc: &Path, active: Vec<String>) -> Result<(), String> {
    let available = list_names(mc);
    let mut entries: Vec<String> = Vec::new();
    for name in active {
        if !available.iter().any(|n| n == &name) {
            return Err(format!("Resource pack desconocido: {name}"));
        }
        let entry = format!("{PREFIX}{name}");
        if !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    let path = options_path(mc);
    let current = fs::read_to_string(&path).unwrap_or_default();
    fs::write(&path, skin::set_pack_entries(&current, KEY, &entries))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estado_activos_primero_y_en_orden() {
        let names = vec!["b.zip".to_string(), "a.zip".to_string(), "c".to_string()];
        let active = vec!["c".to_string(), "b.zip".to_string()];
        let st = merge_state(names, &active);
        assert_eq!(
            st,
            vec![
                PackState { name: "c".into(), enabled: true },
                PackState { name: "b.zip".into(), enabled: true },
                PackState { name: "a.zip".into(), enabled: false },
            ]
        );
    }

    #[test]
    fn estado_ignora_packs_que_no_existen() {
        let names = vec!["a.zip".to_string()];
        let active = vec!["fantasma.zip".to_string(), "a.zip".to_string()];
        let st = merge_state(names, &active);
        assert_eq!(st, vec![PackState { name: "a.zip".into(), enabled: true }]);
    }

    #[test]
    fn escribe_orden_completo() {
        let options = "fov:0.5\nresourcePacks:[\"file/viejo.zip\"]\n";
        let out = skin::set_pack_entries(
            options,
            KEY,
            &["file/b.zip".to_string(), "file/a.zip".to_string()],
        );
        assert_eq!(out, "fov:0.5\nresourcePacks:[\"file/b.zip\",\"file/a.zip\"]\n");
    }

    #[test]
    fn crea_renglon_si_falta() {
        let out = skin::set_pack_entries("", KEY, &["file/a.zip".to_string()]);
        assert_eq!(out, "resourcePacks:[\"file/a.zip\"]");
    }
}
