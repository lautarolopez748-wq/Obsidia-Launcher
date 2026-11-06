//! Resolución centralizada de `loader_version`.
//!
//! Devuelve la versión del loader que debe usarse para un par (loader, versión
//! de Minecraft). Es la única fuente de verdad: `install_fabric` e
//! `install_forge` la consultan cuando el llamador no provee una versión
//! explícita, y el frontend puede usarla antes de guardar un perfil.
//!
//! NeoForge y Quilt se listan para compatibilidad futura pero no tienen
//! instalador propio todavía.

use crate::check_mc_version;
use reqwest::blocking::Client;

/// Loaders que el launcher sabe resolver. Se usa para validar de forma temprana
/// cualquier loader antes de intentar resolverlo.
pub const SUPPORTED_LOADERS: &[&str] = &["Vanilla", "Fabric", "Forge", "NeoForge", "Quilt"];

/// Resuelve la versión del loader para un par (loader, versión de Minecraft).
///
/// - Vanilla → `Ok(None)` (no hay loader).
/// - Fabric → consulta `meta.fabricmc.net` y devuelve la primera versión del loader.
/// - Forge → consulta las promociones de Forge (recommended > latest).
/// - NeoForge / Quilt → `Err` indicando que no hay instalador propio aún.
///
/// `mc_version` se valida con `check_mc_version` antes de tocar la red.
pub fn resolve_loader_version(loader: &str, mc_version: &str) -> Result<Option<String>, String> {
    check_mc_version(mc_version)?;

    if !SUPPORTED_LOADERS.contains(&loader) {
        return Err(format!("Loader no soportado: {loader}"));
    }

    match loader {
        "Vanilla" => Ok(None),
        "Fabric" => resolve_fabric(mc_version),
        "Forge" => resolve_forge(mc_version),
        "NeoForge" | "Quilt" => Err(format!(
            "El loader {loader} no tiene instalador propio en esta versión de Obsidia"
        )),
        _ => unreachable!("loader validado contra SUPPORTED_LOADERS"),
    }
}

/// Selecciona la versión del loader de Fabric a partir de un JSON ya obtenido.
/// Función pura: sin red ni disco, testeable sin conexión.
pub(crate) fn select_fabric_version(versions: &[serde_json::Value]) -> Option<String> {
    versions.iter().find_map(|e| {
        e.get("loader")
            .and_then(|l| l.get("version"))
            .and_then(|v| v.as_str())
            .map(String::from)
    })
}

/// Selecciona la versión de Forge a partir de las promociones ya obtenidas.
/// Prioriza `recommended` sobre `latest`. Función pura: sin red ni disco.
pub(crate) fn select_forge_version(promos: &serde_json::Value, mc_version: &str) -> Option<String> {
    promos.get("promos").and_then(|p| {
        p.get(format!("{mc_version}-recommended"))
            .and_then(|v| v.as_str())
            .or_else(|| {
                p.get(format!("{mc_version}-latest"))
                    .and_then(|v| v.as_str())
            })
            .map(String::from)
    })
}

/// Última versión estable del loader de Fabric para una versión de MC.
fn resolve_fabric(mc_version: &str) -> Result<Option<String>, String> {
    let client = Client::builder()
        .user_agent(crate::launcher::UA)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("https://meta.fabricmc.net/v2/versions/loader/{mc_version}");
    let resp = client
        .get(&url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("No se pudo consultar Fabric: {e}"))?;
    let arr: Vec<serde_json::Value> = resp
        .json()
        .map_err(|e| format!("Respuesta de Fabric inválida: {e}"))?;
    select_fabric_version(&arr)
        .ok_or_else(|| format!("Fabric no tiene loader para {mc_version}"))
        .map(Some)
}

/// Versión de Forge recomendada (o más reciente si no hay recomendada).
fn resolve_forge(mc_version: &str) -> Result<Option<String>, String> {
    let client = Client::builder()
        .user_agent(crate::launcher::UA)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let url = "https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json";
    let resp = client
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("No se pudo consultar Forge: {e}"))?;
    let promos: serde_json::Value = resp
        .json()
        .map_err(|e| format!("Respuesta de Forge inválida: {e}"))?;
    select_forge_version(&promos, mc_version)
        .ok_or_else(|| format!("Forge no tiene versión para {mc_version}"))
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_devuelve_none() {
        assert_eq!(resolve_loader_version("Vanilla", "1.21.4").unwrap(), None);
    }

    #[test]
    fn mc_version_invalida_falla() {
        assert!(resolve_loader_version("Fabric", "../x").is_err());
        assert!(resolve_loader_version("Forge", "").is_err());
    }

    #[test]
    fn loader_no_soportado_falla() {
        assert!(resolve_loader_version("RimJob", "1.21.4").is_err());
    }

    #[test]
    fn neoforge_quilt_sin_instalador_falla() {
        assert!(resolve_loader_version("NeoForge", "1.21.4").is_err());
        assert!(resolve_loader_version("Quilt", "1.21.4").is_err());
    }

    #[test]
    fn fabric_devuelve_version_valida() {
        let json = serde_json::json!([
            {"loader": {"version": "0.16.10"}},
            {"loader": {"version": "0.15.11"}}
        ]);
        let arr = json.as_array().unwrap();
        assert_eq!(select_fabric_version(arr), Some("0.16.10".to_string()));
    }

    #[test]
    fn fabric_sin_versiones_devuelve_none() {
        let json: serde_json::Value = serde_json::from_str("[]").unwrap();
        let arr = json.as_array().unwrap();
        assert_eq!(select_fabric_version(arr), None);
    }

    #[test]
    fn forge_prioriza_recommended_sobre_latest() {
        let promos = serde_json::json!({
            "promos": {
                "1.21.4-recommended": "47.2.0",
                "1.21.4-latest": "47.2.1"
            }
        });
        assert_eq!(
            select_forge_version(&promos, "1.21.4"),
            Some("47.2.0".to_string())
        );
    }

    #[test]
    fn forge_usa_latest_sin_recommended() {
        let promos = serde_json::json!({
            "promos": {
                "1.21.4-latest": "47.2.1"
            }
        });
        assert_eq!(
            select_forge_version(&promos, "1.21.4"),
            Some("47.2.1".to_string())
        );
    }

    #[test]
    fn forge_sin_promos_devuelve_none() {
        let promos = serde_json::json!({"promos": {}});
        assert_eq!(select_forge_version(&promos, "1.21.4"), None);
    }
}
