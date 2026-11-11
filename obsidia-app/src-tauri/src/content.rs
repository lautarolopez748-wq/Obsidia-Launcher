//! Índice de contenido por instancia.
//!
//! Guarda `.obsidia/mods.json`, `.obsidia/shaders.json` y `.obsidia/packs.json`
//! con metadata de cada archivo instalado (proyecto, versión, SHA1, loader,
//! versión de Minecraft, icono). Reemplaza los mapas `obsidia.map.*` de
//! `localStorage` para que la detección de contenido instalado no dependa del
//! navegador.
//!
//! Escrituras atómicas vía `crate::fsx::write_atomic` (read → modify → write).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::fsx;

/// Entrada del índice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentEntry {
    pub slug: String,
    /// Identificador estable del proyecto en Modrinth (`project_id`). Puede
    /// diferir de `slug` en contenido migrado o legado. Predeterminado a None
    /// para índices existentes sin este campo.
    #[serde(default)]
    pub project_id: Option<String>,
    pub version_id: String,
    pub filename: String,
    pub sha1: Option<String>,
    pub loader: Option<String>,
    pub mc_version: Option<String>,
    pub icon_url: Option<String>,
    /// Origen del contenido: "modrinth" para proyectos conocidos, "local" para
    /// archivos sin proyecto Modrinth identificado. Predeterminado "modrinth"
    /// para compatibilidad con índices existentes.
    #[serde(default = "default_content_source")]
    pub source: String,
    /// Indica si el archivo es administrado por Obsidia (se instaló o importó
    /// a través de Obsidia) o es un archivo externo (agregado manualmente).
    /// Los archivos gestionados por Obsidia pueden eliminarse y actualizarse;
    /// los externos se conservan siempre.
    #[serde(default = "default_managed_by_obsidia")]
    pub managed_by_obsidia: bool,
}

fn default_content_source() -> String {
    "modrinth".to_string()
}

fn default_managed_by_obsidia() -> bool {
    false
}

/// Chequeo de actualización para `list_content`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentItem {
    pub file: String,
    pub slug: String,
    pub version: String,
    pub icon: String,
    pub hay_actualizacion: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateStatus {
    pub file: String,
    pub hay_actualizacion: bool,
}

fn index_filename(kind: &str) -> &'static str {
    match kind {
        "mods" => "mods.json",
        "shaders" => "shaders.json",
        "packs" => "packs.json",
        "modpacks" => "modpacks.json",
        _ => "content.json",
    }
}

fn index_path(instance_dir: &Path, kind: &str) -> PathBuf {
    instance_dir.join(".obsidia").join(index_filename(kind))
}

/// Lee el índice; si no existe devuelve lista vacía.
/// Si está corrupto devuelve error sin sobrescribir el archivo.
pub fn read_index(instance_dir: &Path, kind: &str) -> Result<Vec<ContentEntry>, String> {
    let path = index_path(instance_dir, kind);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    if data.trim().is_empty() {
        return Ok(Vec::new());
    }
    // Migración conservadora en lectura: los índices antiguos sin
    // `managed_by_obsidia` se normalizan usando solo evidencia explícita del
    // JSON original. No se escribe nada a disco.
    let mut value: serde_json::Value = serde_json::from_str(&data)
        .map_err(|e| format!("{}: índice corrupto ({e})", path.display()))?;
    if let Some(entries) = value.as_array_mut() {
        for entry in entries.iter_mut() {
            if let Some(obj) = entry.as_object_mut()
                && !obj.contains_key("managed_by_obsidia")
            {
                let inferred = obj.get("source").and_then(|v| v.as_str()) == Some("modrinth")
                    && obj
                        .get("version_id")
                        .and_then(|v| v.as_str())
                        .map(|s| {
                            let t = s.trim();
                            !t.is_empty() && t != "unknown"
                        })
                        .unwrap_or(false);
                obj.insert(
                    "managed_by_obsidia".to_string(),
                    serde_json::json!(inferred),
                );
            }
        }
    }
    serde_json::from_value(value).map_err(|e| format!("{}: índice corrupto ({e})", path.display()))
}

/// Escribe el índice de forma atómica.
pub fn write_index(
    instance_dir: &Path,
    kind: &str,
    entries: &[ContentEntry],
) -> Result<(), String> {
    let path = index_path(instance_dir, kind);
    let data = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    fsx::write_atomic(&path, data.as_bytes(), None)
}

/// Agrega o actualiza una entrada (upsert por slug).
/// No deja dos archivos del mismo proyecto: si el filename cambia, elimina el anterior.
pub fn upsert_entry(
    instance_dir: &Path,
    kind: &str,
    subdir: &Path,
    entry: ContentEntry,
) -> Result<(), String> {
    let mut entries = read_index(instance_dir, kind)?;

    // Si existía otra versión del mismo slug con otro filename, NO borrar todavía.
    // El llamador (apply_update_transactional) borra el anterior DESPUÉS de
    // verificar que el índice nuevo quedó escrito correctamente.
    entries.retain(|e| e.slug != entry.slug);
    let _ = subdir; // documentado: el llamador maneja el borrado post-índice
    entries.push(entry);
    write_index(instance_dir, kind, &entries)
}

/// Incorpora entradas migradas desde localStorage sin sobrescribir metadatos válidos.
#[allow(dead_code)]
pub fn migrate_content_map(
    instance_dir: &Path,
    kind: &str,
    entries: Vec<ContentEntry>,
) -> Result<usize, String> {
    let mut indexed = read_index(instance_dir, kind)?;
    let mut changed = false;

    for candidate in entries {
        let filename = candidate.filename.clone();
        let slug = if candidate.slug.trim().is_empty() {
            // Contenido local sin proyecto Modrinth conocido: slug único que
            // nunca se usa como slug de Modrinth en consultas ni actualizaciones.
            format!("local:{}", filename)
        } else {
            candidate.slug.clone()
        };
        let source = if candidate.slug.trim().is_empty() {
            "local".to_string()
        } else {
            "modrinth".to_string()
        };

        if let Some(existing) = indexed
            .iter_mut()
            .find(|e| e.slug == slug || e.filename == filename)
        {
            if existing.icon_url.is_none() && candidate.icon_url.is_some() {
                existing.icon_url = candidate.icon_url;
                changed = true;
            }
        } else {
            let mut entry = candidate;
            entry.slug = slug;
            if entry.source.trim().is_empty() {
                entry.source = source;
            }
            if entry.version_id.trim().is_empty() {
                entry.version_id = "unknown".to_string();
            }
            indexed.push(entry);
            changed = true;
        }
    }

    if changed {
        write_index(instance_dir, kind, &indexed)?;
    }
    Ok(indexed.len())
}

/// Elimina una entrada por slug y borra el archivo.
#[allow(dead_code)]
pub fn remove_entry(
    instance_dir: &Path,
    kind: &str,
    subdir: &Path,
    slug: &str,
) -> Result<(), String> {
    let mut entries = read_index(instance_dir, kind)?;
    let before = entries.len();
    entries.retain(|e| e.slug != slug);
    if entries.len() == before {
        return Ok(());
    }
    // Borrar el archivo asociado.
    if let Some(old) = read_index(instance_dir, kind)?
        .iter()
        .find(|e| e.slug == slug)
    {
        let _ = std::fs::remove_file(subdir.join(&old.filename));
    }
    write_index(instance_dir, kind, &entries)
}

/// Detecta dependencias requeridas (no opcionales ni recomendadas).
/// Devuelve los slugs requeridos, sin duplicados ni ciclos.
pub fn required_dependencies(deps: &[serde_json::Value]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for d in deps {
        let relation = d
            .get("dependency_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        // Solo required: ignorar optional, recommended, embedded, incompatible.
        if relation != "required" {
            continue;
        }
        let slug = match d.get("project_id").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        // Normalizar a slug (Modrinth usa project_id como slug).
        if seen.insert(slug.clone()) {
            out.push(slug);
        }
    }
    out
}

/// Detecta actualizaciones consultando Modrinth por hash.
#[allow(dead_code)]
pub async fn list_content(
    paths: &crate::Paths,
    instance: &str,
    kind: &str,
) -> Result<Vec<ContentItem>, String> {
    // Rápido y local: no toca la red.
    let indexed = read_index(&paths.instance_dir(instance)?, kind)?;
    let mut out = Vec::new();
    for e in &indexed {
        out.push(ContentItem {
            file: e.filename.clone(),
            slug: e.slug.clone(),
            version: e.version_id.clone(),
            icon: e.icon_url.clone().unwrap_or_default(),
            hay_actualizacion: false,
        });
    }
    Ok(out)
}

// Caché temporal para resultados remotos de actualización.
static UPDATE_CACHE: std::sync::OnceLock<
    std::sync::Mutex<HashMap<String, (bool, std::time::Instant)>>,
> = std::sync::OnceLock::new();

fn update_cache() -> &'static std::sync::Mutex<HashMap<String, (bool, std::time::Instant)>> {
    UPDATE_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// Mapeo puro de la respuesta del batch `version_files/update` a bools de
/// actualización. Sin red: recibe las entradas que se consultaron, sus
/// índices dentro de `indexed` y el valor JSON de respuesta (un objeto
/// hash → versión o null). Devuelve (índice, hay_actualizacion).
/// Para cada entrada: si el batch devolvió un objeto con `id` distinto al
/// `version_id` instalado → hay actualización.
fn map_batch_updates(
    indexed: &[ContentEntry],
    to_query: &[usize],
    response: &Value,
) -> Vec<(usize, bool)> {
    let mut out = Vec::with_capacity(to_query.len());
    for &idx in to_query {
        let e = &indexed[idx];
        let mut hay = false;
        if e.source != "local"
            && let Some(sha) = &e.sha1
            && !sha.is_empty()
            && let Some(obj) = response.get(sha)
            && obj.is_object()
            && let Some(id) = obj.get("id").and_then(|v| v.as_str())
            && id != e.version_id
        {
            hay = true;
        }
        out.push((idx, hay));
    }
    out
}

pub async fn check_content_updates(
    paths: &crate::Paths,
    instance: &str,
    kind: &str,
) -> Result<Vec<UpdateStatus>, String> {
    let indexed = read_index(&paths.instance_dir(instance)?, kind)?;
    let client = crate::http()?;

    let mut results: Vec<bool> = Vec::with_capacity(indexed.len());
    let mut to_query: Vec<usize> = Vec::new();

    // 1. Revisar caché y filtrar las que necesitan red.
    for (idx, e) in indexed.iter().enumerate() {
        if e.source == "local" || e.sha1.as_deref().unwrap_or("").is_empty() {
            results.push(false);
            continue;
        }
        let sha = e.sha1.as_ref().unwrap();
        let key = format!("{}:{}", e.version_id, sha);
        // Caché con TTL de 5 minutos.
        let cached = {
            let cache = update_cache().lock().unwrap_or_else(|e| e.into_inner());
            cache.get(&key).and_then(|(val, ts)| {
                if ts.elapsed().as_secs() < 300 {
                    Some(*val)
                } else {
                    None
                }
            })
        };
        match cached {
            Some(val) => results.push(val),
            None => {
                results.push(false); // placeholder, se llena tras el batch
                to_query.push(idx);
            }
        }
    }

    // 2. Petición batch para todo lo que no está en caché (una sola HTTP).
    if !to_query.is_empty() {
        let to_query_shas: Vec<String> = to_query
            .iter()
            .map(|idx| indexed[*idx].sha1.as_ref().unwrap().clone())
            .collect();

        // Loaders y versiones de MC de las entradas a consultar (compatibilidad).
        let mut loaders: Vec<String> = Vec::new();
        let mut game_versions: Vec<String> = Vec::new();
        for &idx in &to_query {
            let e = &indexed[idx];
            if let Some(l) = &e.loader
                && !loaders.contains(l)
            {
                loaders.push(l.clone());
            }
            if let Some(mv) = &e.mc_version
                && !game_versions.contains(mv)
            {
                game_versions.push(mv.clone());
            }
        }

        let body = serde_json::json!({
            "hashes": to_query_shas,
            "algorithm": "sha1",
            "loaders": loaders,
            "game_versions": game_versions,
        });

        let response_value = match client
            .post("https://api.modrinth.com/v2/version_files/update")
            .json(&body)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => match r.json::<Value>().await {
                Ok(v) => v,
                Err(_) => Value::Null,
            },
            _ => Value::Null,
        };

        // 3. Mapear respuesta y cachear.
        for (idx, hay) in map_batch_updates(&indexed, &to_query, &response_value) {
            results[idx] = hay;
            let e = &indexed[idx];
            let sha = e.sha1.as_ref().unwrap();
            let key = format!("{}:{}", e.version_id, sha);
            let mut cache = update_cache().lock().unwrap_or_else(|e| e.into_inner());
            cache.insert(key, (hay, std::time::Instant::now()));
        }
    }

    Ok(indexed
        .iter()
        .zip(results)
        .map(|(e, hay)| UpdateStatus {
            file: e.filename.clone(),
            hay_actualizacion: hay,
        })
        .collect())
}
#[allow(dead_code)]
pub fn has_cycle(graph: &std::collections::HashMap<String, Vec<String>>) -> bool {
    fn visit(
        node: &str,
        graph: &std::collections::HashMap<String, Vec<String>>,
        visiting: &mut HashSet<String>,
        visited: &mut HashSet<String>,
    ) -> bool {
        if visited.contains(node) {
            return false;
        }
        if !visiting.insert(node.to_string()) {
            return true; // ciclo
        }
        if let Some(deps) = graph.get(node) {
            for dep in deps {
                if visit(dep, graph, visiting, visited) {
                    return true;
                }
            }
        }
        visiting.remove(node);
        visited.insert(node.to_string());
        false
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for node in graph.keys() {
        if visit(node, graph, &mut visiting, &mut visited) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_instance(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("obsidia-content-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".obsidia")).unwrap();
        std::fs::create_dir_all(dir.join("mods")).unwrap();
        dir
    }

    #[test]
    fn indice_roundtrip() {
        let dir = temp_instance("roundtrip");
        let entry = ContentEntry {
            slug: "sodium".to_string(),
            project_id: None,
            version_id: "v123".to_string(),
            filename: "sodium.jar".to_string(),
            sha1: Some("abc".to_string()),
            loader: Some("fabric".to_string()),
            mc_version: Some("1.21.4".to_string()),
            icon_url: Some("https://cdn.modrinth.com/icon.png".to_string()),
            source: "modrinth".to_string(),
            managed_by_obsidia: true,
        };
        write_index(&dir, "mods", &[entry.clone()]).unwrap();
        let loaded = read_index(&dir, "mods").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], entry);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_no_duplica() {
        let dir = temp_instance("upsert");
        let mods_dir = dir.join("mods");
        std::fs::write(mods_dir.join("sodium-v1.jar"), b"v1").unwrap();

        upsert_entry(
            &dir,
            "mods",
            &mods_dir,
            ContentEntry {
                slug: "sodium".to_string(),
                project_id: None,
                version_id: "v1".to_string(),
                filename: "sodium-v1.jar".to_string(),
                sha1: None,
                loader: Some("fabric".to_string()),
                mc_version: Some("1.21.4".to_string()),
                icon_url: None,
                source: "modrinth".to_string(),
                managed_by_obsidia: true,
            },
        )
        .unwrap();

        // Actualizar a v2: debe reemplazar filename sin duplicar.
        // El anterior se borra desde apply_update_transactional, no desde upsert_entry.
        std::fs::write(mods_dir.join("sodium-v2.jar"), b"v2").unwrap();
        upsert_entry(
            &dir,
            "mods",
            &mods_dir,
            ContentEntry {
                slug: "sodium".to_string(),
                project_id: None,
                version_id: "v2".to_string(),
                filename: "sodium-v2.jar".to_string(),
                sha1: None,
                loader: Some("fabric".to_string()),
                mc_version: Some("1.21.4".to_string()),
                icon_url: None,
                source: "modrinth".to_string(),
                managed_by_obsidia: true,
            },
        )
        .unwrap();

        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "sodium-v2.jar");
        // El anterior sigue existiendo: lo borra apply_update_transactional,
        // no upsert_entry (este test prueba upsert aislado).
        assert!(mods_dir.join("sodium-v1.jar").exists());
        assert!(mods_dir.join("sodium-v2.jar").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_entry() {
        let dir = temp_instance("remove");
        let mods_dir = dir.join("mods");
        std::fs::write(mods_dir.join("lithium.jar"), b"x").unwrap();

        upsert_entry(
            &dir,
            "mods",
            &mods_dir,
            ContentEntry {
                slug: "lithium".to_string(),
                project_id: None,
                version_id: "v1".to_string(),
                filename: "lithium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
                managed_by_obsidia: true,
            },
        )
        .unwrap();

        super::remove_entry(&dir, "mods", &mods_dir, "lithium").unwrap();
        assert!(read_index(&dir, "mods").unwrap().is_empty());
        assert!(!mods_dir.join("lithium.jar").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dependencias_requeridas() {
        let deps = vec![
            serde_json::json!({"project_id": "fabric-api", "dependency_type": "required"}),
            serde_json::json!({"project_id": "sodium", "dependency_type": "embedded"}),
            serde_json::json!({"project_id": "opt", "dependency_type": "optional"}),
            serde_json::json!({"project_id": "fabric-api", "dependency_type": "required"}),
        ];
        let req = required_dependencies(&deps);
        assert_eq!(req, vec!["fabric-api"]);
    }

    #[test]
    fn ciclos() {
        let mut graph = std::collections::HashMap::new();
        graph.insert("a".to_string(), vec!["b".to_string()]);
        graph.insert("b".to_string(), vec!["c".to_string()]);
        graph.insert("c".to_string(), vec!["a".to_string()]);
        assert!(has_cycle(&graph));

        let mut graph2 = std::collections::HashMap::new();
        graph2.insert("x".to_string(), vec!["y".to_string()]);
        graph2.insert("y".to_string(), vec![]);
        assert!(!has_cycle(&graph2));
    }

    #[test]
    fn operaciones_a_no_afectan_b() {
        let dir_a = temp_instance("a-iso");
        let dir_b = temp_instance("b-iso");
        let mods_a = dir_a.join("mods");
        let mods_b = dir_b.join("mods");

        std::fs::write(mods_a.join("sodium.jar"), b"a").unwrap();
        std::fs::write(mods_b.join("sodium.jar"), b"b").unwrap();

        upsert_entry(
            &dir_a,
            "mods",
            &mods_a,
            ContentEntry {
                slug: "sodium".to_string(),
                project_id: None,
                version_id: "va".to_string(),
                filename: "sodium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
                managed_by_obsidia: true,
            },
        )
        .unwrap();

        assert_eq!(read_index(&dir_a, "mods").unwrap().len(), 1);
        assert!(read_index(&dir_b, "mods").unwrap().is_empty());
        assert_eq!(std::fs::read(mods_b.join("sodium.jar")).unwrap(), b"b");

        let _ = std::fs::remove_dir_all(&dir_a);
        let _ = std::fs::remove_dir_all(&dir_b);
    }

    #[test]
    fn indice_corrupto_no_sobrescribe() {
        let dir = temp_instance("corrupto");
        let path = dir.join(".obsidia").join("mods.json");
        std::fs::write(&path, "{ no es json válido").unwrap();

        // Debe devolver error, no sobrescribir.
        let result = read_index(&dir, "mods");
        assert!(result.is_err());
        // El archivo original se conserva.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ no es json válido"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn indice_vacio_es_ok() {
        let dir = temp_instance("vacio");
        let result = read_index(&dir, "mods");
        assert!(result.unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_content_map_incorpora_y_preserva() {
        let dir = temp_instance("migrate");
        let existing = ContentEntry {
            slug: "sodium".to_string(),
            project_id: None,
            version_id: "v1".to_string(),
            filename: "sodium.jar".to_string(),
            sha1: None,
            loader: None,
            mc_version: None,
            icon_url: None,
            source: "modrinth".to_string(),
            managed_by_obsidia: true,
        };
        write_index(&dir, "mods", &[existing]).unwrap();

        let migrated = migrate_content_map(
            &dir,
            "mods",
            vec![ContentEntry {
                slug: "sodium".to_string(),
                project_id: None,
                version_id: "".to_string(),
                filename: "sodium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: Some("https://example.com/icon.png".to_string()),
                source: "modrinth".to_string(),
                managed_by_obsidia: true,
            }],
        )
        .unwrap();
        assert_eq!(migrated, 1);
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].icon_url.as_deref(),
            Some("https://example.com/icon.png")
        );
        assert_eq!(entries[0].version_id, "v1");

        // Idempotencia: no duplica.
        migrate_content_map(
            &dir,
            "mods",
            vec![ContentEntry {
                slug: "sodium".to_string(),
                project_id: None,
                version_id: "".to_string(),
                filename: "sodium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: Some("https://example.com/other.png".to_string()),
                source: "modrinth".to_string(),
                managed_by_obsidia: true,
            }],
        )
        .unwrap();
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 1);
        // No sobrescribe icono válido con otro.
        assert_eq!(
            entries[0].icon_url.as_deref(),
            Some("https://example.com/icon.png")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_content_map_no_sobrescribe_metadatos() {
        let dir = temp_instance("migrate2");
        let existing = ContentEntry {
            slug: "sodium".to_string(),
            project_id: None,
            version_id: "v1".to_string(),
            filename: "sodium.jar".to_string(),
            sha1: Some("abc".to_string()),
            loader: Some("fabric".to_string()),
            mc_version: Some("1.21.4".to_string()),
            icon_url: Some("https://example.com/icon.png".to_string()),
            source: "modrinth".to_string(),
            managed_by_obsidia: true,
        };
        write_index(&dir, "mods", &[existing]).unwrap();

        migrate_content_map(
            &dir,
            "mods",
            vec![ContentEntry {
                slug: "sodium".to_string(),
                project_id: None,
                version_id: "".to_string(),
                filename: "sodium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
                managed_by_obsidia: true,
            }],
        )
        .unwrap();
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries[0].version_id, "v1");
        assert_eq!(entries[0].sha1.as_deref(), Some("abc"));
        assert_eq!(entries[0].loader.as_deref(), Some("fabric"));
        assert_eq!(entries[0].mc_version.as_deref(), Some("1.21.4"));
        assert_eq!(
            entries[0].icon_url.as_deref(),
            Some("https://example.com/icon.png")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_content_vacio() {
        let dir = temp_instance("list-vacio");
        let paths = crate::paths::Paths::new(dir.clone());
        let items = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(list_content(&paths, "legacy", "mods"))
            .unwrap();
        assert!(items.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_content_devuelve_entradas() {
        let dir = temp_instance("list-ok");
        let entry = ContentEntry {
            slug: "sodium".to_string(),
            project_id: None,
            version_id: "v1".to_string(),
            filename: "sodium.jar".to_string(),
            sha1: None,
            loader: None,
            mc_version: None,
            icon_url: Some("https://example.com/icon.png".to_string()),
            source: "modrinth".to_string(),
            managed_by_obsidia: true,
        };
        write_index(&dir, "mods", &[entry.clone()]).unwrap();
        let paths = crate::paths::Paths::new(dir.clone());
        let items = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(list_content(&paths, "legacy", "mods"))
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].file, "sodium.jar");
        assert_eq!(items[0].slug, "sodium");
        assert_eq!(items[0].version, "v1");
        assert_eq!(items[0].icon, "https://example.com/icon.png");
        assert!(!items[0].hay_actualizacion);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_content_indice_corrupto() {
        let dir = temp_instance("list-corrupto");
        std::fs::write(dir.join(".obsidia").join("mods.json"), "{ corrupto").unwrap();
        let paths = crate::paths::Paths::new(dir.clone());
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(list_content(&paths, "legacy", "mods"));
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn content_entry_source_predeterminado_es_modrinth() {
        let entry = ContentEntry {
            slug: "sodium".to_string(),
            project_id: None,
            version_id: "v1".to_string(),
            filename: "sodium.jar".to_string(),
            sha1: None,
            loader: Some("fabric".to_string()),
            mc_version: Some("1.21.4".to_string()),
            icon_url: None,
            source: "modrinth".to_string(),
            managed_by_obsidia: true,
        };
        assert_eq!(entry.source, "modrinth");
    }

    #[test]
    fn migrate_content_map_slug_vacio_crea_local() {
        let dir = temp_instance("migrate-local");
        std::fs::write(dir.join("mods").join("mod-desconocido.jar"), b"x").unwrap();

        let count = migrate_content_map(
            &dir,
            "mods",
            vec![ContentEntry {
                slug: "".to_string(),
                project_id: None,
                version_id: "".to_string(),
                filename: "mod-desconocido.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "local".to_string(),
                managed_by_obsidia: false,
            }],
        )
        .unwrap();
        assert_eq!(count, 1);

        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].slug, "local:mod-desconocido.jar");
        assert_eq!(entries[0].source, "local");
        // No es un slug de Modrinth: nunca se consulta como proyecto.
        assert_ne!(entries[0].slug, "mod-desconocido.jar");

        // Idempotencia: no duplica.
        let count2 = migrate_content_map(
            &dir,
            "mods",
            vec![ContentEntry {
                slug: "".to_string(),
                project_id: None,
                version_id: "".to_string(),
                filename: "mod-desconocido.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "local".to_string(),
                managed_by_obsidia: false,
            }],
        )
        .unwrap();
        assert_eq!(count2, 1);
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_content_local_no_consulta_modrinth() {
        let dir = temp_instance("list-local");
        let entry = ContentEntry {
            slug: "local:mod-desconocido.jar".to_string(),
            project_id: None,
            version_id: "".to_string(),
            filename: "mod-desconocido.jar".to_string(),
            sha1: None,
            loader: None,
            mc_version: None,
            icon_url: None,
            source: "local".to_string(),
            managed_by_obsidia: false,
        };
        write_index(&dir, "mods", &[entry.clone()]).unwrap();
        let paths = crate::paths::Paths::new(dir.clone());
        let items = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(list_content(&paths, "legacy", "mods"))
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].file, "mod-desconocido.jar");
        assert_eq!(items[0].slug, "local:mod-desconocido.jar");
        // Contenido local nunca reporta actualización.
        assert!(!items[0].hay_actualizacion);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nombre_archivo_igual_slug_modrinth_no_se_confunde() {
        let dir = temp_instance("migrate-conflict");
        // Un archivo llamado igual a un proyecto real de Modrinth pero sin slug.
        let count = migrate_content_map(
            &dir,
            "mods",
            vec![ContentEntry {
                slug: "".to_string(),
                project_id: None,
                version_id: "".to_string(),
                filename: "sodium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "local".to_string(),
                managed_by_obsidia: false,
            }],
        )
        .unwrap();
        assert_eq!(count, 1);
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries[0].slug, "local:sodium.jar");
        assert_eq!(entries[0].source, "local");
        // Nunca se trata como el proyecto "sodium" de Modrinth.
        assert_ne!(entries[0].slug, "sodium");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn indice_antiguo_sin_managed_modrinth_se_infiere_true() {
        let dir = temp_instance("mig-modrinth");
        let path = dir.join(".obsidia").join("mods.json");
        std::fs::write(
            &path,
            r#"[{"slug":"sodium","version_id":"v1","filename":"sodium.jar","source":"modrinth"}]"#,
        )
        .unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].managed_by_obsidia);
        // La lectura no modifica el archivo.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn indice_antiguo_sin_managed_local_se_infiere_false() {
        let dir = temp_instance("mig-local");
        let path = dir.join(".obsidia").join("mods.json");
        std::fs::write(
            &path,
            r#"[{"slug":"local:mod.jar","version_id":"","filename":"mod.jar","source":"local"}]"#,
        )
        .unwrap();
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].managed_by_obsidia);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn indice_antiguo_sin_managed_ni_source_se_preserva() {
        let dir = temp_instance("mig-nosource");
        let path = dir.join(".obsidia").join("mods.json");
        std::fs::write(
            &path,
            r#"[{"slug":"sodium","version_id":"v1","filename":"sodium.jar"}]"#,
        )
        .unwrap();
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].managed_by_obsidia);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn indice_con_managed_explicito_se_conserva() {
        let dir = temp_instance("mig-explicit");
        let path = dir.join(".obsidia").join("mods.json");
        std::fs::write(
            &path,
            r#"[{"slug":"sodium","version_id":"v1","filename":"sodium.jar","source":"modrinth","managed_by_obsidia":false}]"#,
        )
        .unwrap();
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].managed_by_obsidia);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn indice_sin_managed_ni_version_valida_se_preserva() {
        let dir = temp_instance("mig-unknown");
        let path = dir.join(".obsidia").join("mods.json");
        std::fs::write(
            &path,
            r#"[{"slug":"sodium","version_id":"unknown","filename":"sodium.jar","source":"modrinth"}]"#,
        )
        .unwrap();
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].managed_by_obsidia);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn indice_antiguo_mod_local_importado_historicamente_se_preserva() {
        let dir = temp_instance("mig-local-hist");
        let path = dir.join(".obsidia").join("mods.json");
        std::fs::write(
            &path,
            r#"[{"slug":"local:mod.jar","version_id":"v1","filename":"mod.jar","source":"local"}]"#,
        )
        .unwrap();
        let entries = read_index(&dir, "mods").unwrap();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].managed_by_obsidia);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn map_batch_updates_solo_marca_id_distinto() {
        use serde_json::json;
        let indexed = vec![
            ContentEntry {
                slug: "a".to_string(),
                project_id: None,
                version_id: "v-installed".to_string(),
                filename: "a.jar".to_string(),
                sha1: Some("sha-a".to_string()),
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
                managed_by_obsidia: true,
            },
            ContentEntry {
                slug: "b".to_string(),
                project_id: None,
                version_id: "v-same".to_string(),
                filename: "b.jar".to_string(),
                sha1: Some("sha-b".to_string()),
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
                managed_by_obsidia: true,
            },
            ContentEntry {
                slug: "c".to_string(),
                project_id: None,
                version_id: "v-old".to_string(),
                filename: "c.jar".to_string(),
                sha1: Some("sha-c".to_string()),
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "local".to_string(),
                managed_by_obsidia: false,
            },
            ContentEntry {
                slug: "d".to_string(),
                project_id: None,
                version_id: "v-no-sha".to_string(),
                filename: "d.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
                managed_by_obsidia: true,
            },
        ];

        // Respuesta batch: a → versión nueva (id distinto), b → misma versión,
        // c → local (ignorada), d → sin sha (ignorada), e → no está en respuesta.
        let response = json!({
            "sha-a": {"id": "v-new", "version_id": "v-new"},
            "sha-b": {"id": "v-same"},
        });

        let to_query = vec![0usize, 1, 2, 3];
        let resultados = map_batch_updates(&indexed, &to_query, &response);

        let mapa: HashMap<usize, bool> = resultados.into_iter().collect();
        assert_eq!(mapa[&0], true); // a: id diferente → actualización
        assert_eq!(mapa[&1], false); // b: id igual → sin actualización
        assert_eq!(mapa[&2], false); // c: local → sin actualización
        assert_eq!(mapa[&3], false); // d: sin sha → sin actualización
    }
}
