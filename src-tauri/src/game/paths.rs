//! Carpetas del launcher y comprobación de rutas.

use std::path::{Path, PathBuf};

/// Carpetas donde vive todo lo que el launcher instala.
/// En Windows cuelgan de `%LOCALAPPDATA%\com.meca.launcher`.
#[derive(Debug, Clone)]
pub struct Dirs {
    root: PathBuf,
}

impl Dirs {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Java descargado de Mojang.
    pub fn runtime(&self) -> PathBuf {
        self.root.join("runtime")
    }

    /// Minecraft y Forge: versiones, librerías y recursos.
    pub fn minecraft(&self) -> PathBuf {
        self.root.join("minecraft")
    }

    pub fn versions(&self) -> PathBuf {
        self.minecraft().join("versions")
    }

    pub fn libraries(&self) -> PathBuf {
        self.minecraft().join("libraries")
    }

    pub fn assets(&self) -> PathBuf {
        self.minecraft().join("assets")
    }

    pub fn natives(&self, version_id: &str) -> PathBuf {
        self.minecraft().join("natives").join(version_id)
    }

    /// Carpeta de juego: mods, mundos, opciones y capturas.
    pub fn instance(&self) -> PathBuf {
        self.root.join("instance")
    }

    /// Archivos de trabajo del launcher: instalador de Forge y registros.
    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn version_json(&self, id: &str) -> PathBuf {
        self.versions().join(id).join(format!("{id}.json"))
    }

    pub fn version_jar(&self, id: &str) -> PathBuf {
        self.versions().join(id).join(format!("{id}.jar"))
    }
}

/// Nombres que Windows reserva para dispositivos.
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "conin$", "conout$", "com1", "com2", "com3", "com4", "com5",
    "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8",
    "lpt9",
];

/// `true` si `name` sirve como nombre de archivo o carpeta sin sorpresas:
/// ni vacío, ni `.` o `..`, ni con caracteres que Windows prohíbe.
pub fn is_safe_component(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." || name.len() > 200 {
        return false;
    }
    if name.ends_with('.') || name.ends_with(' ') || name.starts_with(' ') {
        return false;
    }
    if name
        .chars()
        .any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*' | '\\' | '/'))
    {
        return false;
    }
    // Windows ignora los espacios antes de la extensión ("nul .txt" es "nul")
    // y también reserva COM y LPT seguidos de ¹, ² o ³.
    let stem = name.split('.').next().unwrap_or(name).trim_end().to_lowercase();
    let stem = stem.replace('¹', "1").replace('²', "2").replace('³', "3");
    !RESERVED.contains(&stem.as_str())
}

/// Une `relative` (con `/` como separador) a `base`, pero solo si no puede
/// salirse de `base`. Las rutas llegan de internet: nunca se usan sin pasar por aquí.
pub fn safe_join(base: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty() || relative.len() > 500 {
        return None;
    }
    let mut path = base.to_path_buf();
    for component in relative.split('/') {
        if !is_safe_component(component) {
            return None;
        }
        path.push(component);
    }
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ordinary_relative_paths() {
        let base = Path::new("base");
        assert_eq!(
            safe_join(base, "mods/curios-forge-5.14.1+1.20.1.jar"),
            Some(base.join("mods").join("curios-forge-5.14.1+1.20.1.jar"))
        );
        assert!(safe_join(base, "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar").is_some());
        assert!(safe_join(base, "legal/java.base/ADDITIONAL_LICENSE_INFO").is_some());
    }

    #[test]
    fn rejects_paths_that_could_escape_or_confuse_windows() {
        let base = Path::new("base");
        for bad in [
            "",
            "../evil.jar",
            "mods/../../evil.jar",
            "/etc/passwd",
            "C:/Windows/evil.dll",
            "mods\\evil.jar",
            "mods//evil.jar",
            "mods/evil.jar/",
            "mods/./evil.jar",
            "mods/nul.jar",
            "mods/CON",
            "mods/evil.jar.",
            "mods/evil\u{0}.jar",
            "mods/a:b.jar",
            "mods/nul .txt",
            "mods/CONIN$",
            "mods/COM¹.txt",
        ] {
            assert_eq!(safe_join(base, bad), None, "debería rechazar {bad:?}");
        }
    }
}
