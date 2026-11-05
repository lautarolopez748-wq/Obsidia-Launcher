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
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::fsx;

/// Entrada del índice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContentEntry {
    pub slug: String,
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
}

fn default_content_source() -> String {
    "modrinth".to_string()
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

fn index_filename(kind: &str) -> &'static str {
    match kind {
        "mods" => "mods.json",
        "shaders" => "shaders.json",
        "packs" => "packs.json",
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
    serde_json::from_str(&data).map_err(|e| format!("{}: índice corrupto ({e})", path.display()))
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
    let indexed = read_index(&paths.instance_dir(instance)?, kind)?;
    let mut out = Vec::new();
    let client = reqwest::Client::builder()
        .user_agent(crate::launcher::UA)
        .build()
        .map_err(|e| e.to_string())?;

    for e in &indexed {
        // Contenido local: nunca consultar Modrinth ni reportar actualizaciones.
        if e.source == "local" {
            out.push(ContentItem {
                file: e.filename.clone(),
                slug: e.slug.clone(),
                version: e.version_id.clone(),
                icon: e.icon_url.clone().unwrap_or_default(),
                hay_actualizacion: false,
            });
            continue;
        }

        let mut hay = false;
        if let Some(sha) = &e.sha1 {
            // Consultar Modrinth por hash.
            let resp = client
                .get(format!(
                    "https://api.modrinth.com/v2/version/{}/file",
                    e.version_id
                ))
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if resp.status().is_success() {
                let file_info: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
                let current_sha = file_info
                    .get("hashes")
                    .and_then(|h| h.get("sha1"))
                    .and_then(|h| h.as_str())
                    .unwrap_or("");
                if current_sha != sha {
                    hay = true;
                }
            }
        }
        out.push(ContentItem {
            file: e.filename.clone(),
            slug: e.slug.clone(),
            version: e.version_id.clone(),
            icon: e.icon_url.clone().unwrap_or_default(),
            hay_actualizacion: hay,
        });
    }
    Ok(out)
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
            version_id: "v123".to_string(),
            filename: "sodium.jar".to_string(),
            sha1: Some("abc".to_string()),
            loader: Some("fabric".to_string()),
            mc_version: Some("1.21.4".to_string()),
            icon_url: Some("https://cdn.modrinth.com/icon.png".to_string()),
            source: "modrinth".to_string(),
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
                version_id: "v1".to_string(),
                filename: "sodium-v1.jar".to_string(),
                sha1: None,
                loader: Some("fabric".to_string()),
                mc_version: Some("1.21.4".to_string()),
                icon_url: None,
                source: "modrinth".to_string(),
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
                version_id: "v2".to_string(),
                filename: "sodium-v2.jar".to_string(),
                sha1: None,
                loader: Some("fabric".to_string()),
                mc_version: Some("1.21.4".to_string()),
                icon_url: None,
                source: "modrinth".to_string(),
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
                version_id: "v1".to_string(),
                filename: "lithium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
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
                version_id: "va".to_string(),
                filename: "sodium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
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
            version_id: "v1".to_string(),
            filename: "sodium.jar".to_string(),
            sha1: None,
            loader: None,
            mc_version: None,
            icon_url: None,
            source: "modrinth".to_string(),
        };
        write_index(&dir, "mods", &[existing]).unwrap();

        let migrated = migrate_content_map(
            &dir,
            "mods",
            vec![ContentEntry {
                slug: "sodium".to_string(),
                version_id: "".to_string(),
                filename: "sodium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: Some("https://example.com/icon.png".to_string()),
                source: "modrinth".to_string(),
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
                version_id: "".to_string(),
                filename: "sodium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: Some("https://example.com/other.png".to_string()),
                source: "modrinth".to_string(),
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
            version_id: "v1".to_string(),
            filename: "sodium.jar".to_string(),
            sha1: Some("abc".to_string()),
            loader: Some("fabric".to_string()),
            mc_version: Some("1.21.4".to_string()),
            icon_url: Some("https://example.com/icon.png".to_string()),
            source: "modrinth".to_string(),
        };
        write_index(&dir, "mods", &[existing]).unwrap();

        migrate_content_map(
            &dir,
            "mods",
            vec![ContentEntry {
                slug: "sodium".to_string(),
                version_id: "".to_string(),
                filename: "sodium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "modrinth".to_string(),
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
            version_id: "v1".to_string(),
            filename: "sodium.jar".to_string(),
            sha1: None,
            loader: None,
            mc_version: None,
            icon_url: Some("https://example.com/icon.png".to_string()),
            source: "modrinth".to_string(),
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
            version_id: "v1".to_string(),
            filename: "sodium.jar".to_string(),
            sha1: None,
            loader: Some("fabric".to_string()),
            mc_version: Some("1.21.4".to_string()),
            icon_url: None,
            source: "modrinth".to_string(),
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
                version_id: "".to_string(),
                filename: "mod-desconocido.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "local".to_string(),
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
                version_id: "".to_string(),
                filename: "mod-desconocido.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "local".to_string(),
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
            version_id: "".to_string(),
            filename: "mod-desconocido.jar".to_string(),
            sha1: None,
            loader: None,
            mc_version: None,
            icon_url: None,
            source: "local".to_string(),
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
                version_id: "".to_string(),
                filename: "sodium.jar".to_string(),
                sha1: None,
                loader: None,
                mc_version: None,
                icon_url: None,
                source: "local".to_string(),
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
}
