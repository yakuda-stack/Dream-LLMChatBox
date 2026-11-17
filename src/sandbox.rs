use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Folder {
    pub path: String,
    #[serde(default)]
    pub writable: bool,
}

const SYSTEM_DIRS: [&str; 14] = [
    "/", "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/proc", "/root", "/run", "/sbin",
    "/sys", "/usr", "/var",
];

const SECRET_NAMES: [&str; 6] = [
    ".ssh",
    ".gnupg",
    ".password-store",
    ".pki",
    ".aws",
    "keyrings",
];

pub fn to_json(folders: &[Folder]) -> String {
    if folders.is_empty() {
        String::new()
    } else {
        serde_json::to_string(folders).unwrap_or_default()
    }
}

pub fn from_json(text: &str) -> Vec<Folder> {
    serde_json::from_str(text).unwrap_or_default()
}

pub fn display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

pub fn check_root(path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !canonical.is_dir() {
        return Err(format!("{} is not a folder", canonical.display()));
    }
    if SYSTEM_DIRS.iter().any(|dir| canonical == Path::new(dir)) {
        return Err(format!("{} is a system folder", canonical.display()));
    }
    if dirs::home_dir().is_some_and(|home| canonical == home) {
        return Err("The whole home folder can't be shared, pick a project folder".to_string());
    }
    if is_secret(&canonical) {
        return Err(format!("{} contains secrets", canonical.display()));
    }
    Ok(canonical)
}

fn is_secret(path: &Path) -> bool {
    path.components().any(|part| match part {
        Component::Normal(name) => SECRET_NAMES.contains(&name.to_string_lossy().as_ref()),
        _ => false,
    })
}

pub struct Resolved {
    pub path: PathBuf,
    pub root: PathBuf,
    pub writable: bool,
}

impl Resolved {
    pub fn shown(&self) -> String {
        let name = display_name(&self.root.to_string_lossy());
        match self.path.strip_prefix(&self.root) {
            Ok(rest) if rest.as_os_str().is_empty() => name,
            Ok(rest) => format!("{name}/{}", rest.display()),
            Err(_) => self.path.display().to_string(),
        }
    }
}

pub fn resolve(folders: &[Folder], requested: &str) -> Result<Resolved, String> {
    let first = folders.first().ok_or("No folder is shared in this chat")?;
    let requested = requested.trim();
    let wanted = if requested.is_empty() || requested == "." {
        PathBuf::from(&first.path)
    } else if let Some(rest) = requested.strip_prefix("~/") {
        dirs::home_dir().unwrap_or_default().join(rest)
    } else if Path::new(requested).is_absolute() {
        PathBuf::from(requested)
    } else {
        let by_name = folders.iter().find_map(|folder| {
            let name = display_name(&folder.path);
            let rest = requested.strip_prefix(&name)?;
            (rest.is_empty() || rest.starts_with('/'))
                .then(|| PathBuf::from(&folder.path).join(rest.trim_start_matches('/')))
        });
        by_name
            .filter(|path| path.exists())
            .unwrap_or_else(|| PathBuf::from(&first.path).join(requested))
    };

    let path = canonical_or_planned(&wanted)?;
    if is_secret(&path) {
        return Err("Access to secret files is not allowed".to_string());
    }
    for folder in folders {
        let Ok(root) = Path::new(&folder.path).canonicalize() else {
            continue;
        };
        if path.starts_with(&root) {
            return Ok(Resolved {
                path,
                root,
                writable: folder.writable,
            });
        }
    }
    Err(format!(
        "{} is outside the shared folders. Shared: {}",
        wanted.display(),
        folders
            .iter()
            .map(|folder| folder.path.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

fn canonical_or_planned(path: &Path) -> Result<PathBuf, String> {
    if let Ok(canonical) = path.canonicalize() {
        return Ok(canonical);
    }
    let mut missing: Vec<&std::ffi::OsStr> = Vec::new();
    let mut existing = path;
    loop {
        if existing.exists() {
            break;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name);
                existing = parent;
            }
            _ => return Err(format!("{} is not a valid path", path.display())),
        }
    }
    if path
        .components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err("Paths with .. are not allowed for new files".to_string());
    }
    let mut planned = existing
        .canonicalize()
        .map_err(|e| format!("{}: {e}", existing.display()))?;
    for name in missing.into_iter().rev() {
        planned.push(name);
    }
    Ok(planned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn stays_inside() {
        let base = std::env::temp_dir().join(format!("dlc-sandbox-{}", std::process::id()));
        let project = base.join("project");
        let outside = base.join("outside");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(project.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(outside.join("secret.txt"), "no").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, project.join("link")).unwrap();

        let folders = vec![Folder {
            path: project.canonicalize().unwrap().to_string_lossy().to_string(),
            writable: false,
        }];
        assert!(resolve(&folders, "src/main.rs").is_ok());
        assert!(resolve(&folders, "project/src/main.rs").is_ok());
        assert!(resolve(&folders, ".").is_ok());
        assert!(resolve(&folders, "new/file.txt").is_ok());
        assert!(resolve(&folders, "../outside/secret.txt").is_err());
        assert!(resolve(&folders, "new/../../outside/x.txt").is_err());
        assert!(resolve(&folders, "/etc/passwd").is_err());
        #[cfg(unix)]
        assert!(resolve(&folders, "link/secret.txt").is_err());
        assert!(resolve(&[], "src/main.rs").is_err());
        assert!(check_root(Path::new("/")).is_err());
        assert!(check_root(Path::new("/etc")).is_err());
        assert!(check_root(&project).is_ok());
        let _ = fs::remove_dir_all(&base);
    }
}
