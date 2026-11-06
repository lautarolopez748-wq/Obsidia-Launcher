//! Resolución pura de dependencias obligatorias de Modrinth (Bloque 4C).
//!
//! Sin red ni disco: recibe los metadatos ya obtenidos (versión compatible
//! elegida por el llamador), la lista de instalados y la versión de
//! Minecraft / loader, y devuelve el plan ordenado de instalación.
//!
//! Reglas:
//! - Solo `required`: se excluyen `optional`, `recommended`, `embedded` e
//!   `incompatible`.
//! - Recursión post-orden: cada dependencia va antes que quien la pide.
//! - Duplicados: un proyecto aparece una sola vez en el plan.
//! - Ciclos: el camino de recursión (visited) detecta el ciclo y falla.
//! - Compatibilidad: la versión candidata debe declarar `mc_version` en
//!   `game_versions` y `loader` en `loaders` (si los listas no están vacíos).
//! - Ya instalada: mismo `slug` + `version_id` en el índice → se marca y el
//!   llamador la omite al descargar.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// Una dependencia tal como la informa Modrinth.
#[derive(Clone, Debug, PartialEq)]
pub struct DependencyMetadata {
    pub project_id: String,
    pub dependency_type: String,
    /// Versión específica requerida (si la hay). Si es None, cualquier versión
    /// instalada del mismo proyecto satisface la dependencia.
    pub version_id: Option<String>,
}

/// Metadatos de UNA versión compatible (la elegida para `mc_version`/`loader`).
#[derive(Clone, Debug, PartialEq)]
pub struct VersionMetadata {
    pub project_id: Option<String>,
    pub version_id: String,
    pub filename: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub dependencies: Vec<DependencyMetadata>,
    /// URL directa del archivo principal de esta versión (extraída una vez en
    /// `collect_candidates` para evitar consultas repetidas al instalar).
    pub file_url: Option<String>,
    /// SHA1 del archivo principal de esta versión (extraído una vez).
    pub file_sha1: Option<String>,
}

/// Entrada mínima del índice: lo que basta para saber si ya está instalado.
#[derive(Clone, Debug, PartialEq)]
pub struct InstalledMod {
    pub slug: String,
    pub project_id: Option<String>,
    pub version_id: String,
    pub mc_version: Option<String>,
    pub loader: Option<String>,
}

/// Una dependencia ordenada para instalar (o para omitir si ya está instalada).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct DependencyPlan {
    pub slug: String,
    pub project_id: Option<String>,
    pub version_id: String,
    pub filename: String,
    pub already_installed: bool,
}

/// Plan completo: mod solicitado + dependencias en orden de instalación.
/// `install_mod` lo compara con el plan confirmado por el usuario.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ModPlan {
    pub slug: String,
    pub version_id: String,
    pub filename: String,
    pub dependencies: Vec<DependencyPlan>,
}

fn is_compatible(meta: &VersionMetadata, mc_version: &str, loader: &str) -> bool {
    if !meta.game_versions.is_empty()
        && !meta
            .game_versions
            .iter()
            .any(|v| v.eq_ignore_ascii_case(mc_version))
    {
        return false;
    }
    if !meta.loaders.is_empty()
        && loader != "Vanilla"
        && !meta.loaders.iter().any(|l| l.eq_ignore_ascii_case(loader))
    {
        return false;
    }
    true
}

fn incompatible_msg(slug: &str, mc_version: &str, loader: &str) -> String {
    format!("No hay una versión compatible de {slug} para Minecraft {mc_version} ({loader})")
}

#[allow(clippy::too_many_arguments)]
fn visit(
    slug: &str,
    candidates: &HashMap<String, VersionMetadata>,
    mc_version: &str,
    loader: &str,
    // Versión requerida por la dependencia padre (None = cualquier versión instalada sirve).
    required_version: Option<&str>,
    installed: &[InstalledMod],
    visited: &mut HashSet<String>,
    seen: &mut HashSet<String>,
    plan: &mut Vec<DependencyPlan>,
) -> Result<(), String> {
    if !visited.insert(slug.to_string()) {
        return Err(format!("Ciclo de dependencias detectado en {slug}"));
    }
    let meta = candidates
        .get(slug)
        .ok_or_else(|| incompatible_msg(slug, mc_version, loader))?;
    if !is_compatible(meta, mc_version, loader) {
        return Err(incompatible_msg(slug, mc_version, loader));
    }

    for dep in &meta.dependencies {
        if dep.dependency_type != "required" {
            continue;
        }
        let dep_slug = &dep.project_id;
        if visited.contains(dep_slug) {
            return Err(format!("Ciclo de dependencias detectado en {dep_slug}"));
        }
        if seen.contains(dep_slug) {
            continue;
        }
        visit(
            dep_slug,
            candidates,
            mc_version,
            loader,
            dep.version_id.as_deref(),
            installed,
            visited,
            seen,
            plan,
        )?;
    }

    if seen.insert(slug.to_string()) {
        let is_installed = installed.iter().any(|e| {
            // Identidad estable: project_id si existe, slug como fallback.
            let meta_identity = meta.project_id.as_deref().unwrap_or(slug);
            let installed_identity = e.project_id.as_deref().unwrap_or(&e.slug);
            let identity_match = installed_identity == meta_identity;
            if !identity_match {
                return false;
            }
            // La versión instalada debe coincidir con la requerida (si la hay)
            // o, en su defecto, con la versión candidata para este perfil.
            let expected_version = required_version.unwrap_or(&meta.version_id);
            if e.version_id != *expected_version {
                return false;
            }
            // Verificar compatibilidad con el perfil actual.
            // Metadatos antiguos (sin mc_version/loader) no se asumen compatibles.
            let mc_match = e.mc_version.as_ref().map_or(false, |v| v == mc_version);
            let loader_match = e.loader.as_ref().map_or(false, |v| v == loader);
            mc_match && loader_match
        });
        plan.push(DependencyPlan {
            slug: slug.to_string(),
            project_id: meta.project_id.clone(),
            version_id: meta.version_id.clone(),
            filename: meta.filename.clone(),
            already_installed: is_installed,
        });
    }
    visited.remove(slug);
    Ok(())
}

/// Resuelve las dependencias obligatorias del mod raíz.
///
/// `candidates` contiene un `VersionMetadata` por slug (raíz incluida): la
/// versión compatible ya elegida para `mc_version`/`loader`. Un slug ausente
/// (o incompatible) es un error, no un plan parcial.
pub fn resolve_dependencies(
    root_slug: &str,
    candidates: &HashMap<String, VersionMetadata>,
    mc_version: &str,
    loader: &str,
    installed: &[InstalledMod],
) -> Result<Vec<DependencyPlan>, String> {
    let mut visited = HashSet::new();
    visited.insert(root_slug.to_string());
    let mut seen = HashSet::new();
    let mut plan = Vec::new();

    let root = candidates
        .get(root_slug)
        .ok_or_else(|| incompatible_msg(root_slug, mc_version, loader))?;
    if !is_compatible(root, mc_version, loader) {
        return Err(incompatible_msg(root_slug, mc_version, loader));
    }

    for dep in &root.dependencies {
        if dep.dependency_type != "required" {
            continue;
        }
        let dep_slug = &dep.project_id;
        if visited.contains(dep_slug) {
            return Err(format!("Ciclo de dependencias detectado en {dep_slug}"));
        }
        if seen.contains(dep_slug) {
            continue;
        }
        visit(
            dep_slug,
            candidates,
            mc_version,
            loader,
            dep.version_id.as_deref(),
            installed,
            &mut visited,
            &mut seen,
            &mut plan,
        )?;
    }
    Ok(plan)
}

/// Arma el plan completo (mod principal + dependencias) desde los metadatos.
pub fn build_mod_plan(
    root_slug: &str,
    candidates: &HashMap<String, VersionMetadata>,
    mc_version: &str,
    loader: &str,
    installed: &[InstalledMod],
) -> Result<ModPlan, String> {
    let root = candidates
        .get(root_slug)
        .ok_or_else(|| incompatible_msg(root_slug, mc_version, loader))?;
    if !is_compatible(root, mc_version, loader) {
        return Err(incompatible_msg(root_slug, mc_version, loader));
    }
    let dependencies = resolve_dependencies(root_slug, candidates, mc_version, loader, installed)?;
    Ok(ModPlan {
        slug: root_slug.to_string(),
        version_id: root.version_id.clone(),
        filename: root.filename.clone(),
        dependencies,
    })
}

/// ¿El plan confirmado por el usuario coincide con el recién calculado?
/// Compara mod principal y cada dependencia (slug, versión, archivo y estado).
pub fn plan_matches(expected: &ModPlan, actual: &ModPlan) -> bool {
    expected == actual
}

#[cfg(test)]
mod tests {
    use super::*;

    const MC: &str = "1.21.4";
    const LOADER: &str = "fabric";

    fn meta(version_id: &str, filename: &str, deps: &[(&str, &str)]) -> VersionMetadata {
        VersionMetadata {
            project_id: None,
            version_id: version_id.to_string(),
            filename: filename.to_string(),
            game_versions: vec![MC.to_string()],
            loaders: vec![LOADER.to_string()],
            dependencies: deps
                .iter()
                .map(|(p, t)| DependencyMetadata {
                    project_id: p.to_string(),
                    dependency_type: t.to_string(),
                    version_id: None,
                })
                .collect(),
            file_url: None,
            file_sha1: None,
        }
    }

    fn candidates(entries: Vec<(&str, VersionMetadata)>) -> HashMap<String, VersionMetadata> {
        entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect()
    }

    fn slugs(plan: &[DependencyPlan]) -> Vec<&str> {
        plan.iter().map(|d| d.slug.as_str()).collect()
    }

    #[test]
    fn dependencia_simple() {
        let c = candidates(vec![
            ("main", meta("v-main", "main.jar", &[("lib", "required")])),
            ("lib", meta("v-lib", "lib.jar", &[])),
        ]);
        let plan = resolve_dependencies("main", &c, MC, LOADER, &[]).unwrap();
        assert_eq!(slugs(&plan), vec!["lib"]);
        assert_eq!(plan[0].version_id, "v-lib");
        assert_eq!(plan[0].filename, "lib.jar");
        assert!(!plan[0].already_installed);
    }

    #[test]
    fn dependencias_anidadas_en_orden() {
        let c = candidates(vec![
            ("main", meta("v-main", "main.jar", &[("a", "required")])),
            ("a", meta("v-a", "a.jar", &[("b", "required")])),
            ("b", meta("v-b", "b.jar", &[])),
        ]);
        let plan = resolve_dependencies("main", &c, MC, LOADER, &[]).unwrap();
        assert_eq!(slugs(&plan), vec!["b", "a"]);
    }

    #[test]
    fn orden_instalacion_dependencia_antes_que_dependiente() {
        let c = candidates(vec![
            (
                "main",
                meta(
                    "v-main",
                    "main.jar",
                    &[("a", "required"), ("c", "required")],
                ),
            ),
            ("a", meta("v-a", "a.jar", &[("b", "required")])),
            ("b", meta("v-b", "b.jar", &[])),
            ("c", meta("v-c", "c.jar", &[])),
        ]);
        let plan = resolve_dependencies("main", &c, MC, LOADER, &[]).unwrap();
        assert_eq!(slugs(&plan), vec!["b", "a", "c"]);
    }

    #[test]
    fn dependencias_duplicadas_se_dedup() {
        // Diamante: main -> a, main -> b, a -> c, b -> c.
        let c = candidates(vec![
            (
                "main",
                meta(
                    "v-main",
                    "main.jar",
                    &[("a", "required"), ("b", "required")],
                ),
            ),
            ("a", meta("v-a", "a.jar", &[("c", "required")])),
            ("b", meta("v-b", "b.jar", &[("c", "required")])),
            ("c", meta("v-c", "c.jar", &[])),
        ]);
        let plan = resolve_dependencies("main", &c, MC, LOADER, &[]).unwrap();
        assert_eq!(slugs(&plan), vec!["c", "a", "b"]);
        assert_eq!(plan.iter().filter(|d| d.slug == "c").count(), 1);
    }

    #[test]
    fn ciclo_detectado() {
        let c = candidates(vec![
            ("main", meta("v-main", "main.jar", &[("a", "required")])),
            ("a", meta("v-a", "a.jar", &[("main", "required")])),
        ]);
        let err = resolve_dependencies("main", &c, MC, LOADER, &[]).unwrap_err();
        assert!(err.contains("Ciclo"), "{err}");

        // Auto-ciclo.
        let c2 = candidates(vec![
            ("main", meta("v-main", "main.jar", &[("a", "required")])),
            ("a", meta("v-a", "a.jar", &[("a", "required")])),
        ]);
        let err = resolve_dependencies("main", &c2, MC, LOADER, &[]).unwrap_err();
        assert!(err.contains("Ciclo"), "{err}");
    }

    #[test]
    fn dependencia_ya_instalada_se_marca() {
        let c = candidates(vec![
            ("main", meta("v-main", "main.jar", &[("lib", "required")])),
            ("lib", meta("v-lib", "lib.jar", &[])),
        ]);

        // Versión instalada coincide: cuenta como instalada.
        let installed = vec![InstalledMod {
            slug: "lib".to_string(),
            project_id: Some("lib".to_string()),
            version_id: "v-lib".to_string(),
            mc_version: Some(MC.to_string()),
            loader: Some(LOADER.to_string()),
        }];
        let plan = resolve_dependencies("main", &c, MC, LOADER, &installed).unwrap();
        assert!(plan[0].already_installed);

        // Otra versión instalada no cuenta: versión no coincide.
        let installed_other = vec![InstalledMod {
            slug: "lib".to_string(),
            project_id: None,
            version_id: "v-otra".to_string(),
            mc_version: Some(MC.to_string()),
            loader: Some(LOADER.to_string()),
        }];
        let plan = resolve_dependencies("main", &c, MC, LOADER, &installed_other).unwrap();
        assert!(!plan[0].already_installed);

        // Sin mc_version/loader en la entrada instalada: no se asume compatible.
        let installed_sin_meta = vec![InstalledMod {
            slug: "lib".to_string(),
            project_id: None,
            version_id: "v-lib".to_string(),
            mc_version: None,
            loader: None,
        }];
        let plan = resolve_dependencies("main", &c, MC, LOADER, &installed_sin_meta).unwrap();
        assert!(!plan[0].already_installed);

        // Si la dependencia especifica una versión concreta, debe coincidir.
        let c_con_version = candidates(vec![
            (
                "main",
                VersionMetadata {
                    project_id: None,
                    version_id: "v-main".to_string(),
                    filename: "main.jar".to_string(),
                    game_versions: vec![MC.to_string()],
                    loaders: vec![LOADER.to_string()],
                    dependencies: vec![DependencyMetadata {
                        project_id: "lib".to_string(),
                        dependency_type: "required".to_string(),
                        version_id: Some("v-lib".to_string()),
                    }],
                    file_url: None,
                    file_sha1: None,
                },
            ),
            (
                "lib",
                VersionMetadata {
                    project_id: None,
                    version_id: "v-lib".to_string(),
                    filename: "lib.jar".to_string(),
                    game_versions: vec![MC.to_string()],
                    loaders: vec![LOADER.to_string()],
                    dependencies: vec![],
                    file_url: None,
                    file_sha1: None,
                },
            ),
        ]);
        let installed_otra = vec![InstalledMod {
            slug: "lib".to_string(),
            project_id: None,
            version_id: "v-otra".to_string(),
            mc_version: Some(MC.to_string()),
            loader: Some(LOADER.to_string()),
        }];
        let plan =
            resolve_dependencies("main", &c_con_version, MC, LOADER, &installed_otra).unwrap();
        assert!(!plan[0].already_installed);

        let plan = resolve_dependencies("main", &c_con_version, MC, LOADER, &installed).unwrap();
        assert!(plan[0].already_installed);
    }

    #[test]
    fn excluye_opcionales_y_recomendadas() {
        let c = candidates(vec![
            (
                "main",
                meta(
                    "v-main",
                    "main.jar",
                    &[
                        ("opt", "optional"),
                        ("rec", "recommended"),
                        ("emb", "embedded"),
                        ("inc", "incompatible"),
                        ("req", "required"),
                    ],
                ),
            ),
            ("opt", meta("v-opt", "opt.jar", &[])),
            ("rec", meta("v-rec", "rec.jar", &[])),
            ("emb", meta("v-emb", "emb.jar", &[])),
            ("inc", meta("v-inc", "inc.jar", &[])),
            ("req", meta("v-req", "req.jar", &[])),
        ]);
        let plan = resolve_dependencies("main", &c, MC, LOADER, &[]).unwrap();
        assert_eq!(slugs(&plan), vec!["req"]);
    }

    #[test]
    fn incompatibilidad_minecraft_y_loader() {
        let mut lib = meta("v-lib", "lib.jar", &[]);
        lib.game_versions = vec!["1.20.1".to_string()];
        let c = candidates(vec![
            ("main", meta("v-main", "main.jar", &[("lib", "required")])),
            ("lib", lib),
        ]);
        let err = resolve_dependencies("main", &c, MC, LOADER, &[]).unwrap_err();
        assert!(err.contains("compatible"), "{err}");

        let mut lib2 = meta("v-lib", "lib.jar", &[]);
        lib2.loaders = vec!["forge".to_string()];
        let c2 = candidates(vec![
            ("main", meta("v-main", "main.jar", &[("lib", "required")])),
            ("lib", lib2),
        ]);
        let err = resolve_dependencies("main", &c2, MC, LOADER, &[]).unwrap_err();
        assert!(err.contains("compatible"), "{err}");

        // Raíz incompatible también es error.
        let mut root = meta("v-main", "main.jar", &[]);
        root.game_versions = vec!["1.19.2".to_string()];
        let c3 = candidates(vec![("main", root)]);
        let err = build_mod_plan("main", &c3, MC, LOADER, &[]).unwrap_err();
        assert!(err.contains("compatible"), "{err}");
    }

    #[test]
    fn candidato_faltante_es_error() {
        let c = candidates(vec![(
            "main",
            meta("v-main", "main.jar", &[("lib", "required")]),
        )]);
        let err = resolve_dependencies("main", &c, MC, LOADER, &[]).unwrap_err();
        assert!(err.contains("lib"), "{err}");
    }

    #[test]
    fn plan_coincide_solo_si_es_identico() {
        let c = candidates(vec![
            ("main", meta("v-main", "main.jar", &[("lib", "required")])),
            ("lib", meta("v-lib", "lib.jar", &[])),
        ]);
        let plan = build_mod_plan("main", &c, MC, LOADER, &[]).unwrap();
        assert!(plan_matches(&plan, &plan.clone()));

        let mut changed = plan.clone();
        changed.dependencies[0].version_id = "v-otra".to_string();
        assert!(!plan_matches(&plan, &changed));

        let mut changed_root = plan.clone();
        changed_root.version_id = "v-main-2".to_string();
        assert!(!plan_matches(&plan, &changed_root));
    }
}
