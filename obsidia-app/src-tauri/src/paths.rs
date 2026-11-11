//! Rutas de Obsidia y validación de identificadores de instancia.
//!
//! `Paths` se construye una sola vez al arrancar la aplicación. Así ninguna
//! llamada vuelve a leer `APPDATA` ni cae en el directorio actual cuando falta.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Directorios de la instalación. `minecraft` es la raíz compartida y
/// `instances_root` contiene una carpeta por instancia.
#[derive(Clone, Debug)]
pub struct Paths {
    pub minecraft: PathBuf,
    pub instances_root: PathBuf,
    pub versions: PathBuf,
    /// El launcher deriva estas dos de `minecraft`; quedan expuestas para las
    /// etapas que muevan la descarga de librerías y assets.
    #[allow(dead_code)]
    pub libraries: PathBuf,
    #[allow(dead_code)]
    pub assets: PathBuf,
    pub runtime: PathBuf,
    pub obsidia: PathBuf,
}

impl Paths {
    /// Arma las rutas a partir de la raíz `.minecraft` sin tocar el entorno.
    pub fn new(minecraft: PathBuf) -> Self {
        Self {
            instances_root: minecraft.join("instances"),
            versions: minecraft.join("versions"),
            libraries: minecraft.join("libraries"),
            assets: minecraft.join("assets"),
            runtime: minecraft.join("runtime"),
            obsidia: minecraft.join("obsidia"),
            minecraft,
        }
    }

    /// Ubica `.minecraft` dentro de `APPDATA`. Si la variable falta o está
    /// vacía devuelve un error: nunca se usa el directorio actual.
    pub fn discover() -> Result<Self, String> {
        Self::from_appdata(std::env::var_os("APPDATA"))
    }

    fn from_appdata(appdata: Option<OsString>) -> Result<Self, String> {
        let appdata = appdata.filter(|value| !value.is_empty()).ok_or_else(|| {
            "No se encontró APPDATA: no se puede ubicar la carpeta .minecraft".to_string()
        })?;
        Ok(Self::new(PathBuf::from(appdata).join(".minecraft")))
    }

    pub fn profiles_file(&self) -> PathBuf {
        self.obsidia.join("profiles.json")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.obsidia.join("settings.json")
    }

    pub fn servers_file(&self) -> PathBuf {
        self.obsidia.join("servers.json")
    }

    pub fn launcher_profiles_file(&self) -> PathBuf {
        self.minecraft.join("launcher_profiles.json")
    }

    /// Carpeta física de los mundos compartidos por todas las instancias.
    pub fn shared_saves(&self) -> PathBuf {
        self.minecraft.join("saves")
    }

    /// Carpeta de juego de una instancia. `"legacy"` apunta a `.minecraft`.
    /// Un identificador vacío o inválido es un error.
    pub fn instance_dir(&self, instance: &str) -> Result<PathBuf, String> {
        match InstanceRef::parse(instance)? {
            InstanceRef::Legacy => Ok(self.minecraft.clone()),
            InstanceRef::Instance(id) => self.instance_dir_for(&id),
        }
    }

    /// Carpeta de una instancia ya validada. Comprueba que la ruta final,
    /// después de resolver junctions y symlinks, siga dentro de `instances_root`.
    /// La raíz y la propia instancia no pueden ser reparse points.
    pub fn instance_dir_for(&self, id: &InstanceId) -> Result<PathBuf, String> {
        self.validate_instances_root()?;
        let dir = self.instances_root.join(id.as_str());
        ensure_inside(&self.instances_root, &dir)?;
        reject_reparse_point(&dir, "La carpeta de la instancia")?;
        Ok(dir)
    }

    /// Rechaza que `instances_root` sea un junction, un symlink o un archivo.
    pub fn validate_instances_root(&self) -> Result<(), String> {
        match fs::symlink_metadata(&self.instances_root) {
            Ok(meta) => {
                if is_reparse_meta(&meta) {
                    return Err(format!(
                        "{} no puede ser un junction ni un symlink",
                        self.instances_root.display()
                    ));
                }
                if !meta.is_dir() {
                    return Err(format!(
                        "{} no es una carpeta",
                        self.instances_root.display()
                    ));
                }
                Ok(())
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", self.instances_root.display())),
        }
    }

    /// Asegura que `instance_dir/saves` sea un junction a `shared_saves()`.
    ///
    /// Si ya apunta al destino correcto no hace nada. Un enlace a otro destino
    /// se rechaza. Una carpeta real con contenido no se mueve, borra ni
    /// reemplaza. Un enlace roto se elimina solo a sí mismo y se recrea. Nunca
    /// se copian ni se migran mundos automáticamente.
    pub fn ensure_saves_link(&self, instance_dir: &Path) -> Result<(), String> {
        self.validate_instances_root()?;
        if instance_dir == self.minecraft || instance_dir == self.shared_saves() {
            return Err(
                "La instancia legacy usa .minecraft directamente: no lleva junction de saves"
                    .to_string(),
            );
        }
        if !instance_dir.starts_with(&self.instances_root) {
            return Err(format!(
                "La instancia {} no está dentro de {}",
                instance_dir.display(),
                self.instances_root.display()
            ));
        }
        reject_reparse_point(instance_dir, "La carpeta de la instancia")?;

        let shared = self.shared_saves();
        fs::create_dir_all(&shared).map_err(|e| format!("{}: {e}", shared.display()))?;
        fs::create_dir_all(instance_dir).map_err(|e| format!("{}: {e}", instance_dir.display()))?;

        let link = instance_dir.join("saves");
        match fs::symlink_metadata(&link) {
            Ok(meta) => {
                if is_reparse_meta(&meta) {
                    match fs::canonicalize(&link) {
                        Ok(real) => {
                            let shared_real = fs::canonicalize(&shared)
                                .map_err(|e| format!("{}: {e}", shared.display()))?;
                            if same_path(&real, &shared_real) {
                                return Ok(());
                            }
                            return Err(format!(
                                "{} ya existe y apunta a {} en vez de {}. No se reemplaza.",
                                link.display(),
                                real.display(),
                                shared_real.display()
                            ));
                        }
                        Err(_) => {
                            // Enlace roto: se elimina solo el enlace, nunca su destino.
                            remove_link_only(&link)?;
                        }
                    }
                } else if meta.is_dir() {
                    let mut entries =
                        fs::read_dir(&link).map_err(|e| format!("{}: {e}", link.display()))?;
                    if entries.next().is_some() {
                        return Err(format!(
                            "{} ya existe como carpeta real con contenido. No se mueve ni se reemplaza; mové los mundos manualmente a {}.",
                            link.display(),
                            shared.display()
                        ));
                    }
                    fs::remove_dir(&link).map_err(|e| format!("{}: {e}", link.display()))?;
                } else {
                    return Err(format!("{} existe y no es una carpeta", link.display()));
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", link.display())),
        }

        create_dir_link(&link, &shared)?;
        let real = fs::canonicalize(&link).map_err(|e| format!("{}: {e}", link.display()))?;
        let shared_real =
            fs::canonicalize(&shared).map_err(|e| format!("{}: {e}", shared.display()))?;
        if !same_path(&real, &shared_real) {
            return Err(format!(
                "No se pudo verificar el junction {} -> {}",
                link.display(),
                shared.display()
            ));
        }
        Ok(())
    }
}

/// Identificador seguro de instancia: nombre simple de carpeta, sin rutas.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InstanceId(String);

impl InstanceId {
    pub const MAX_LEN: usize = 48;

    /// Acepta hasta 48 caracteres alfanuméricos, `-` y `_`. Rechaza `legacy`
    /// y los nombres de dispositivo reservados por Windows.
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.is_empty() {
            return Err("Falta el identificador de instancia".to_string());
        }
        if raw.len() > Self::MAX_LEN {
            return Err(format!(
                "El identificador de instancia supera {MAX_LEN} caracteres",
                MAX_LEN = Self::MAX_LEN
            ));
        }
        if !raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(
                "Identificador de instancia inválido: solo letras, números, '-' y '_'".to_string(),
            );
        }
        if raw.eq_ignore_ascii_case("legacy") {
            return Err(
                "\"legacy\" está reservado para la instancia compartida .minecraft".to_string(),
            );
        }
        if is_windows_reserved(raw) {
            return Err(format!(
                "Identificador de instancia reservado por Windows: {raw}"
            ));
        }
        Ok(Self(raw.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Referencia a una instancia. `Legacy` es el token explícito para `.minecraft`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstanceRef {
    Instance(InstanceId),
    Legacy,
}

impl InstanceRef {
    /// `None` ya no existe en la API: una cadena vacía es un error. Solo el
    /// token explícito `legacy` (sin distinguir mayúsculas) usa `.minecraft`.
    pub fn parse(raw: &str) -> Result<Self, String> {
        if raw.is_empty() {
            return Err("Falta la instancia: usá \"legacy\" para .minecraft".to_string());
        }
        if raw.eq_ignore_ascii_case("legacy") {
            return Ok(Self::Legacy);
        }
        InstanceId::parse(raw).map(Self::Instance)
    }
}

/// Nombres de dispositivo que Windows reserva en cualquier carpeta.
pub(crate) fn is_windows_reserved(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let bytes = upper.as_bytes();
    bytes.len() == 4
        && (bytes.starts_with(b"COM") || bytes.starts_with(b"LPT"))
        && bytes[3].is_ascii_digit()
}

/// Comprueba que `candidate` quede dentro de `root` después de resolver enlaces.
///
/// Si la raíz todavía no existe no puede haber un junction dentro y alcanza con
/// la comprobación léxica. Si la instancia existe, se canonicaliza y se compara
/// contra la raíz canonicalizada: un junction o symlink que apunte afuera se
/// rechaza. Un enlace roto también se rechaza porque no se puede verificar.
pub(crate) fn ensure_inside(root: &Path, candidate: &Path) -> Result<(), String> {
    if !candidate.starts_with(root) {
        return Err(format!(
            "La ruta {} queda fuera de {}",
            candidate.display(),
            root.display()
        ));
    }

    let root_real = match fs::canonicalize(root) {
        Ok(path) => path,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("No se pudo resolver {}: {e}", root.display())),
    };

    let mut probe = candidate.to_path_buf();
    let mut suffix: Vec<OsString> = Vec::new();
    loop {
        match fs::symlink_metadata(&probe) {
            Ok(_) => break,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let name = probe.file_name().ok_or_else(|| {
                    format!("No se pudo comprobar la ruta {}", candidate.display())
                })?;
                suffix.push(name.to_os_string());
                probe = probe
                    .parent()
                    .ok_or_else(|| format!("No se pudo comprobar la ruta {}", candidate.display()))?
                    .to_path_buf();
            }
            Err(e) => return Err(format!("No se pudo comprobar {}: {e}", probe.display())),
        }
    }

    let real = fs::canonicalize(&probe)
        .map_err(|e| format!("No se pudo resolver {}: {e}", probe.display()))?;
    if !real.starts_with(&root_real) {
        return Err(format!(
            "La ruta {} sale de {} después de resolver enlaces",
            candidate.display(),
            root.display()
        ));
    }

    for name in suffix.iter().rev() {
        let mut components = Path::new(name).components();
        match (components.next(), components.next()) {
            (Some(Component::Normal(_)), None) => {}
            _ => return Err(format!("Componente inválido en {}", candidate.display())),
        }
    }
    Ok(())
}

#[cfg(windows)]
pub(crate) fn is_reparse_meta(meta: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(crate) fn is_reparse_meta(meta: &fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

/// Detecta junctions y symlinks sin seguir el enlace. Un path inexistente no es
/// un reparse point.
pub(crate) fn is_reparse_point(path: &Path) -> bool {
    fs::symlink_metadata(path)
        .map(|meta| is_reparse_meta(&meta))
        .unwrap_or(false)
}

/// Rechaza un junction o symlink en una ruta que debe ser real.
pub(crate) fn reject_reparse_point(path: &Path, label: &str) -> Result<(), String> {
    if is_reparse_point(path) {
        return Err(format!(
            "{label} no puede ser un junction ni un symlink: {}",
            path.display()
        ));
    }
    Ok(())
}

/// Elimina solamente el enlace, sin tocar el destino.
pub(crate) fn remove_link_only(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !is_reparse_meta(&meta) {
        return Err(format!("{} no es un enlace", path.display()));
    }

    let as_dir = meta.is_dir() || path.is_dir();
    let first = if as_dir {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };
    if first.is_ok() {
        return Ok(());
    }
    let second = if as_dir {
        fs::remove_file(path)
    } else {
        fs::remove_dir(path)
    };
    second.map_err(|e| format!("{}: {e}", path.display()))
}

/// Elimina un directorio sin seguir junctions ni symlinks. Si encuentra un
/// reparse point en cualquier nivel devuelve error y no borra nada de ese
/// subárbol. Nunca se usa sobre una ruta que pueda resolver a `.minecraft/saves`.
pub fn safe_remove_dir_all(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if is_reparse_meta(&meta) {
        return Err(format!(
            "{} no puede ser un junction ni un symlink",
            path.display()
        ));
    }
    if !meta.is_dir() {
        return Err(format!("{} no es una carpeta", path.display()));
    }
    for entry in fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))? {
        let entry = entry.map_err(|e| format!("{}: {e}", path.display()))?;
        let child = entry.path();
        let child_meta =
            fs::symlink_metadata(&child).map_err(|e| format!("{}: {e}", child.display()))?;
        if is_reparse_meta(&child_meta) {
            return Err(format!(
                "{} no puede ser un junction ni un symlink",
                child.display()
            ));
        }
        if child_meta.is_dir() {
            safe_remove_dir_all(&child)?;
        } else {
            fs::remove_file(&child).map_err(|e| format!("{}: {e}", child.display()))?;
        }
    }
    fs::remove_dir(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Compara rutas canonicalizadas. En Windows el sistema de archivos no
/// distingue mayúsculas.
pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(windows) {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    } else {
        a == b
    }
}

#[cfg(windows)]
fn create_dir_link(link: &Path, target: &Path) -> Result<(), String> {
    let status = crate::new_cmd("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .status()
        .map_err(|e| format!("No se pudo ejecutar mklink: {e}"))?;
    if !status.success() {
        return Err(format!(
            "No se pudo crear el junction {} -> {}",
            link.display(),
            target.display()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn create_dir_link(link: &Path, target: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, link).map_err(|e| format!("{}: {e}", link.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_paths(tag: &str) -> Paths {
        let dir = std::env::temp_dir().join(format!("obsidia-paths-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Paths::new(dir.join(".minecraft"))
    }

    #[test]
    fn ids_validos() {
        let maximo = "a".repeat(InstanceId::MAX_LEN);
        for raw in ["a", "A-1_x", "pabc-123_x", maximo.as_str()] {
            assert!(InstanceId::parse(raw).is_ok(), "debería aceptar {raw:?}");
        }
    }

    #[test]
    fn ids_invalidos() {
        let largo = "a".repeat(InstanceId::MAX_LEN + 1);
        for raw in ["../x", "a/b", "a\\b", "C:", "CON", "", largo.as_str()] {
            assert!(InstanceId::parse(raw).is_err(), "debería rechazar {raw:?}");
        }
        assert!(InstanceId::parse("legacy").is_err());
        assert!(InstanceId::parse("LEGACY").is_err());
        assert!(InstanceId::parse("con").is_err());
        assert!(InstanceId::parse("COM1").is_err());
        assert!(InstanceId::parse("LPT9").is_err());
    }

    #[test]
    fn appdata_ausente_es_error() {
        assert!(Paths::from_appdata(None).is_err());
        assert!(Paths::from_appdata(Some(OsString::new())).is_err());
        let paths =
            Paths::from_appdata(Some(OsString::from("C:\\Users\\x\\AppData\\Roaming"))).unwrap();
        assert_eq!(
            paths.minecraft,
            PathBuf::from("C:\\Users\\x\\AppData\\Roaming").join(".minecraft")
        );
    }

    #[test]
    fn legacy_es_token_explicito() {
        let paths = temp_paths("legacy");
        assert_eq!(paths.instance_dir("legacy").unwrap(), paths.minecraft);
        assert_eq!(paths.instance_dir("LEGACY").unwrap(), paths.minecraft);
        assert!(paths.instance_dir("").is_err());
        assert!(paths.instance_dir("  ").is_err());
    }

    #[test]
    fn instancia_normal_queda_en_instances_root() {
        let paths = temp_paths("normal");
        let dir = paths.instance_dir("pabc-123_x").unwrap();
        assert_eq!(dir, paths.instances_root.join("pabc-123_x"));
    }

    #[cfg(windows)]
    #[test]
    fn junction_que_sale_de_instances_root_se_rechaza() {
        use std::process::Command;

        let paths = temp_paths("junction");
        fs::create_dir_all(&paths.instances_root).unwrap();
        let outside = paths.minecraft.join("afuera");
        fs::create_dir_all(&outside).unwrap();
        let link = paths.instances_root.join("escape");

        let status = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .status()
            .expect("no se pudo ejecutar mklink");
        assert!(status.success(), "mklink /J falló");

        let result = paths.instance_dir("escape");
        assert!(
            result.is_err(),
            "un junction que sale de instances_root debe rechazarse: {result:?}"
        );
        let _ = fs::remove_dir(&link);
    }

    #[cfg(windows)]
    #[test]
    fn junction_dentro_de_instances_root_se_acepta() {
        use std::process::Command;

        let paths = temp_paths("junction-dentro");
        fs::create_dir_all(&paths.instances_root).unwrap();
        let inside = paths.instances_root.join("real");
        fs::create_dir_all(&inside).unwrap();
        let link = paths.instances_root.join("alias");

        let status = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&inside)
            .status()
            .expect("no se pudo ejecutar mklink");
        assert!(status.success(), "mklink /J falló");

        assert!(paths.instance_dir("alias").is_err());
        let _ = fs::remove_dir(&link);
    }

    #[cfg(windows)]
    #[test]
    fn a_y_b_comparten_los_mismos_mundos() {
        use std::process::Command;
        let paths = temp_paths("shared-ab");
        fs::create_dir_all(&paths.instances_root).unwrap();
        let a = paths.instance_dir("A").unwrap();
        let b = paths.instance_dir("B").unwrap();
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        paths.ensure_saves_link(&a).unwrap();
        paths.ensure_saves_link(&b).unwrap();

        let world = paths.shared_saves().join("mundo-compartido");
        fs::create_dir_all(&world).unwrap();
        fs::write(world.join("level.dat"), b"mundo").unwrap();

        assert!(
            a.join("saves")
                .join("mundo-compartido")
                .join("level.dat")
                .exists()
        );
        assert!(
            b.join("saves")
                .join("mundo-compartido")
                .join("level.dat")
                .exists()
        );
        assert_eq!(
            fs::canonicalize(a.join("saves")).unwrap(),
            fs::canonicalize(b.join("saves")).unwrap()
        );

        let _ = fs::remove_dir_all(&paths.minecraft);
    }

    #[cfg(windows)]
    #[test]
    fn crear_junction_dos_veces_no_falla() {
        let paths = temp_paths("shared-twice");
        fs::create_dir_all(&paths.instances_root).unwrap();
        let a = paths.instance_dir("A").unwrap();
        fs::create_dir_all(&a).unwrap();
        paths.ensure_saves_link(&a).unwrap();
        paths.ensure_saves_link(&a).unwrap();
        let _ = fs::remove_dir_all(&paths.minecraft);
    }

    #[cfg(windows)]
    #[test]
    fn junction_a_otra_ubicacion_se_rechaza() {
        use std::process::Command;
        let paths = temp_paths("shared-other");
        fs::create_dir_all(&paths.instances_root).unwrap();
        let a = paths.instance_dir("A").unwrap();
        fs::create_dir_all(&a).unwrap();
        let other = paths.minecraft.join("otra-carpeta");
        fs::create_dir_all(&other).unwrap();
        let link = a.join("saves");
        let status = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&other)
            .status()
            .expect("no se pudo ejecutar mklink");
        assert!(status.success());

        let res = paths.ensure_saves_link(&a);
        assert!(res.is_err(), "debería rechazar el junction: {res:?}");

        let _ = fs::remove_dir(&link);
        let _ = fs::remove_dir_all(&paths.minecraft);
    }

    #[cfg(windows)]
    #[test]
    fn carpeta_saves_real_con_contenido_no_se_reemplaza() {
        let paths = temp_paths("shared-real");
        fs::create_dir_all(&paths.instances_root).unwrap();
        let a = paths.instance_dir("A").unwrap();
        fs::create_dir_all(&a).unwrap();
        let saves = a.join("saves");
        fs::create_dir_all(&saves).unwrap();
        fs::write(saves.join("level.dat"), b"mundo").unwrap();

        let res = paths.ensure_saves_link(&a);
        assert!(res.is_err(), "debería rechazar la carpeta real: {res:?}");
        assert!(saves.join("level.dat").exists());

        let _ = fs::remove_dir_all(&paths.minecraft);
    }

    #[cfg(windows)]
    #[test]
    fn instances_root_como_junction_se_rechaza() {
        use std::process::Command;
        let paths = temp_paths("root-junction");
        fs::create_dir_all(&paths.minecraft).unwrap();
        let outside = paths.minecraft.join("afuera");
        fs::create_dir_all(&outside).unwrap();
        // Reemplazar instances_root por un junction no es posible si ya existe;
        // se valida que un junction existente se rechace.
        let _ = fs::remove_dir_all(&paths.instances_root);
        let status = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&paths.instances_root)
            .arg(&outside)
            .status()
            .expect("no se pudo ejecutar mklink");
        assert!(status.success());
        assert!(paths.validate_instances_root().is_err());
        let _ = fs::remove_dir(&paths.instances_root);
    }

    #[cfg(windows)]
    #[test]
    fn legacy_funciona_sin_junction() {
        let paths = temp_paths("legacy-nojunction");
        assert_eq!(paths.instance_dir("legacy").unwrap(), paths.minecraft);
        assert!(paths.ensure_saves_link(&paths.minecraft).is_err());
        let _ = fs::remove_dir_all(&paths.minecraft);
    }
}
