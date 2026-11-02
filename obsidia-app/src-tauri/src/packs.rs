//! Orden real de los resource packs.
//!
//! Minecraft guarda los packs activos en `options.txt` (`resourcePacks:[...]`).
//! Esa lista está en orden ascendente de prioridad: **la última entrada gana**.
//! La UI muestra lo inverso (arriba = mayor prioridad), así que este módulo
//! invierte el orden al leer y al escribir. Las entradas sin prefijo `file/`
//! (`vanilla`, `fabric`, `quilt`, `neoforge`, `programmer_art`, `high_contrast`)
//! y las entradas `file/` de packs ausentes se conservan tal cual.

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

/// Entradas crudas de la última línea `resourcePacks:` (con o sin prefijo `file/`).
fn read_entries(mc: &Path) -> Result<Vec<String>, String> {
    let content = skin::read_options(&options_path(mc))?;
    let Some(line) = skin::find_pack_line(&content, KEY) else {
        return Ok(Vec::new());
    };
    skin::parse_pack_list(line)
}

/// Estado para la UI: primero los activos en orden de prioridad (el último de
/// `options.txt` es el primero de la lista) y después el resto.
pub fn merge_state(names: Vec<String>, options_order: &[String]) -> Vec<PackState> {
    let available: HashSet<&str> = names.iter().map(String::as_str).collect();
    let mut out: Vec<PackState> = Vec::new();
    for entry in options_order.iter().rev() {
        let Some(name) = entry.strip_prefix(PREFIX) else { continue };
        if available.contains(name) && !out.iter().any(|p| p.name == name) {
            out.push(PackState { name: name.to_string(), enabled: true });
        }
    }
    for name in &names {
        if !out.iter().any(|p| &p.name == name) {
            out.push(PackState { name: name.clone(), enabled: false });
        }
    }
    out
}

pub fn get_state(mc: &Path) -> Result<Vec<PackState>, String> {
    let entries = read_entries(mc)?;
    Ok(merge_state(list_names(mc), &entries))
}

/// El nombre tiene que ser un nombre simple dentro de `resourcepacks/`.
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\'])
        || name.contains('\0')
    {
        return Err(format!("Nombre de resource pack inválido: {name:?}"));
    }
    Ok(())
}

/// ¿La entrada es un pack que este módulo administra (existe en `resourcepacks/`)?
fn is_managed(entry: &str, available: &[String]) -> bool {
    entry
        .strip_prefix(PREFIX)
        .is_some_and(|name| available.iter().any(|n| n == name))
}

/// Reordena solo las entradas administradas; las demás quedan en su lugar.
///
/// `wanted` viene en orden de `options.txt` (menor a mayor prioridad). Si hay
/// más entradas nuevas que huecos, las sobrantes van al final (prioridad máxima).
fn merge_entries(current: &[String], wanted: &[String], available: &[String]) -> Vec<String> {
    let mut pending = wanted.iter();
    let mut out: Vec<String> = Vec::with_capacity(current.len().max(wanted.len()));
    for entry in current {
        if is_managed(entry, available) {
            if let Some(next) = pending.next() {
                out.push(next.clone());
            }
        } else {
            out.push(entry.clone());
        }
    }
    out.extend(pending.cloned());
    out
}

/// Guarda el orden que muestra la UI (arriba = mayor prioridad).
///
/// Conserva las entradas especiales y los packs `file/` que no están en disco.
pub fn save_state(mc: &Path, ui_order: Vec<String>) -> Result<(), String> {
    let available = list_names(mc);
    let mut wanted: Vec<String> = Vec::new();
    for name in ui_order {
        validate_name(&name)?;
        if !available.iter().any(|n| n == &name) {
            return Err(format!("Resource pack desconocido: {name}"));
        }
        let entry = format!("{PREFIX}{name}");
        if !wanted.contains(&entry) {
            wanted.push(entry);
        }
    }
    // La UI manda la mayor prioridad primero; `options.txt` la guarda al final.
    let wanted_options: Vec<String> = wanted.into_iter().rev().collect();

    let path = options_path(mc);
    let current = skin::read_options(&path)?;
    let line = skin::find_pack_line(&current, KEY);
    let entries = match line {
        Some(line) => skin::parse_pack_list(line)?,
        None => Vec::new(),
    };
    let merged = merge_entries(&entries, &wanted_options, &available);
    // Sin renglón y sin nada que activar no hay por qué crear `options.txt`.
    if line.is_none() && merged.is_empty() {
        return Ok(());
    }
    let next = skin::set_pack_entries(&current, KEY, &merged)?;
    if next == current {
        return Ok(());
    }
    skin::write_options(&path, &next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_mc(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "obsidia-packs-{}-{}-{tag}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("resourcepacks")).unwrap();
        dir
    }

    fn pack_file(mc: &Path, name: &str) {
        fs::write(mc.join("resourcepacks").join(name), b"pack").unwrap();
    }

    #[test]
    fn estado_activos_primero_y_en_orden_real() {
        let names = vec!["b.zip".to_string(), "a.zip".to_string(), "c".to_string()];
        // options.txt: la última entrada es la de mayor prioridad.
        let active = vec!["file/c".to_string(), "file/b.zip".to_string()];
        let st = merge_state(names, &active);
        assert_eq!(
            st,
            vec![
                PackState { name: "b.zip".into(), enabled: true },
                PackState { name: "c".into(), enabled: true },
                PackState { name: "a.zip".into(), enabled: false },
            ]
        );
    }

    #[test]
    fn estado_ignora_packs_que_no_existen() {
        let names = vec!["a.zip".to_string()];
        let active = vec!["file/a.zip".to_string(), "file/fantasma.zip".to_string()];
        let st = merge_state(names, &active);
        assert_eq!(st, vec![PackState { name: "a.zip".into(), enabled: true }]);
    }

    #[test]
    fn merge_conserva_especiales_y_huerfanos() {
        let current = vec![
            "vanilla".to_string(),
            "file/A.zip".to_string(),
            "fabric".to_string(),
            "file/B.zip".to_string(),
            "file/Ghost.zip".to_string(),
        ];
        let available = vec!["A.zip".to_string(), "B.zip".to_string()];
        // Mayor prioridad primero: A.
        let wanted = vec!["file/B.zip".to_string(), "file/A.zip".to_string()];
        let out = merge_entries(&current, &wanted, &available);
        assert_eq!(
            out,
            vec![
                "vanilla",
                "file/B.zip",
                "fabric",
                "file/A.zip",
                "file/Ghost.zip",
            ]
        );
    }

    #[test]
    fn merge_quita_solo_los_desactivados() {
        let current = vec![
            "vanilla".to_string(),
            "file/A.zip".to_string(),
            "fabric".to_string(),
            "file/B.zip".to_string(),
            "file/Ghost.zip".to_string(),
        ];
        let available = vec!["A.zip".to_string(), "B.zip".to_string()];
        let out = merge_entries(&current, &["file/A.zip".to_string()], &available);
        assert_eq!(
            out,
            vec!["vanilla", "file/A.zip", "fabric", "file/Ghost.zip"]
        );
    }

    #[test]
    fn guarda_invirtiendo_y_conserva_especiales() {
        let mc = temp_mc("guarda");
        pack_file(&mc, "A.zip");
        pack_file(&mc, "B.zip");
        fs::write(
            mc.join("options.txt"),
            "fov:0.5\nresourcePacks:[\"vanilla\",\"file/A.zip\",\"fabric\",\"file/B.zip\",\"file/Ghost.zip\"]\n",
        )
        .unwrap();

        // UI: A arriba (mayor prioridad), después B.
        save_state(&mc, vec!["A.zip".into(), "B.zip".into()]).unwrap();

        let out = fs::read_to_string(mc.join("options.txt")).unwrap();
        assert_eq!(
            out,
            "fov:0.5\nresourcePacks:[\"vanilla\",\"file/B.zip\",\"fabric\",\"file/A.zip\",\"file/Ghost.zip\"]\n"
        );
        let st = get_state(&mc).unwrap();
        assert_eq!(st[0], PackState { name: "A.zip".into(), enabled: true });
        assert_eq!(st[1], PackState { name: "B.zip".into(), enabled: true });

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn guarda_usa_la_ultima_clave_duplicada() {
        let mc = temp_mc("duplicada");
        pack_file(&mc, "A.zip");
        fs::write(
            mc.join("options.txt"),
            "resourcePacks:[\"file/viejo.zip\"]\nresourcePacks:[\"vanilla\"]\n",
        )
        .unwrap();

        save_state(&mc, vec!["A.zip".into()]).unwrap();

        let out = fs::read_to_string(mc.join("options.txt")).unwrap();
        assert_eq!(
            out,
            "resourcePacks:[\"file/viejo.zip\"]\nresourcePacks:[\"vanilla\",\"file/A.zip\"]\n"
        );

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn guarda_no_toca_nada_si_json_es_invalido() {
        let mc = temp_mc("json");
        pack_file(&mc, "A.zip");
        let original = "resourcePacks:[\"file/A.zip\"\nfov:0.5\n";
        fs::write(mc.join("options.txt"), original).unwrap();

        assert!(save_state(&mc, vec!["A.zip".into()]).is_err());
        assert_eq!(fs::read_to_string(mc.join("options.txt")).unwrap(), original);
        assert!(!mc.join("options.txt.tmp").exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn guarda_no_toca_nada_si_no_puede_leer() {
        let mc = temp_mc("ilegible");
        pack_file(&mc, "A.zip");
        // Un directorio donde va el archivo hace fallar la lectura en todas las plataformas.
        fs::create_dir_all(mc.join("options.txt")).unwrap();

        assert!(save_state(&mc, vec!["A.zip".into()]).is_err());
        assert!(mc.join("options.txt").is_dir());
        assert!(!mc.join("options.txt.tmp").exists());
        assert!(get_state(&mc).is_err());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn save_state_recupera_el_respaldo_huerfano() {
        let mc = temp_mc("recupera-bak");
        pack_file(&mc, "A.zip");
        let previo = "fov:0.5\nresourcePacks:[\"vanilla\"]\n";
        fs::write(mc.join("options.txt.bak"), previo).unwrap();

        save_state(&mc, vec!["A.zip".into()]).unwrap();

        assert_eq!(
            fs::read_to_string(mc.join("options.txt")).unwrap(),
            "fov:0.5\nresourcePacks:[\"vanilla\",\"file/A.zip\"]\n"
        );
        assert_eq!(
            fs::read_to_string(mc.join("options.txt.bak")).unwrap(),
            previo
        );

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn get_state_recupera_el_respaldo_huerfano() {
        let mc = temp_mc("recupera-get");
        pack_file(&mc, "A.zip");
        fs::write(
            mc.join("options.txt.bak"),
            "resourcePacks:[\"file/A.zip\"]\n",
        )
        .unwrap();

        let st = get_state(&mc).unwrap();
        assert_eq!(
            st,
            vec![PackState { name: "A.zip".into(), enabled: true }]
        );
        assert!(mc.join("options.txt").exists());
        assert!(!mc.join("options.txt.bak").exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn rechaza_packs_desconocidos() {
        let mc = temp_mc("desconocido");
        fs::write(mc.join("options.txt"), "resourcePacks:[]\n").unwrap();
        assert!(save_state(&mc, vec!["no-existe.zip".into()]).is_err());
        assert_eq!(
            fs::read_to_string(mc.join("options.txt")).unwrap(),
            "resourcePacks:[]\n"
        );

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn guarda_conserva_crlf_y_sin_salto_final() {
        let mc = temp_mc("crlf");
        pack_file(&mc, "A.zip");
        fs::write(
            mc.join("options.txt"),
            "fov:0.5\r\nresourcePacks:[\"vanilla\"]",
        )
        .unwrap();

        save_state(&mc, vec!["A.zip".into()]).unwrap();

        assert_eq!(
            fs::read_to_string(mc.join("options.txt")).unwrap(),
            "fov:0.5\r\nresourcePacks:[\"vanilla\",\"file/A.zip\"]"
        );

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn deduplica_nombres_repetidos() {
        let mc = temp_mc("duplicados");
        pack_file(&mc, "A.zip");
        fs::write(mc.join("options.txt"), "resourcePacks:[]\n").unwrap();

        save_state(&mc, vec!["A.zip".into(), "A.zip".into()]).unwrap();

        assert_eq!(
            fs::read_to_string(mc.join("options.txt")).unwrap(),
            "resourcePacks:[\"file/A.zip\"]\n"
        );

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn rechaza_nombres_invalidos() {
        let mc = temp_mc("nombres");
        fs::write(mc.join("options.txt"), "resourcePacks:[]\n").unwrap();
        for name in ["", ".", "..", "../fuera.zip", "sub/dir.zip", "sub\\dir.zip"] {
            assert!(save_state(&mc, vec![name.to_string()]).is_err(), "{name:?}");
        }
        assert_eq!(
            fs::read_to_string(mc.join("options.txt")).unwrap(),
            "resourcePacks:[]\n"
        );

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn no_crea_options_si_no_hay_nada_que_guardar() {
        let mc = temp_mc("sin-options");
        save_state(&mc, Vec::new()).unwrap();
        assert!(!mc.join("options.txt").exists());

        let _ = fs::remove_dir_all(&mc);
    }

    #[test]
    fn escribe_orden_completo() {
        let options = "fov:0.5\nresourcePacks:[\"file/viejo.zip\"]\n";
        let out = skin::set_pack_entries(
            options,
            KEY,
            &["file/b.zip".to_string(), "file/a.zip".to_string()],
        )
        .unwrap();
        assert_eq!(out, "fov:0.5\nresourcePacks:[\"file/b.zip\",\"file/a.zip\"]\n");
    }

    #[test]
    fn crea_renglon_si_falta() {
        let out =
            skin::set_pack_entries("", KEY, &["file/a.zip".to_string()]).unwrap();
        assert_eq!(out, "resourcePacks:[\"file/a.zip\"]");
    }
}
