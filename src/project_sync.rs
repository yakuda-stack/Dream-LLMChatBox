use crate::sandbox::Folder;
use crate::workspace;
use serde_json::{Value, json};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const NAME: &str = "sync_from_project";
const LISTED: usize = 30;

pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": "Copy files from the user's original project folder into your test environment, overwriting files that differ there. Use it when a file in the test environment is missing or older than in the project. path is relative to the project; leave it empty for the whole project. Never touches the original project. Runs without asking.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File or folder relative to the project, empty = whole project" }
                },
                "required": []
            }
        }
    })
}

pub fn available(folders: &[Folder]) -> bool {
    pair(folders).is_some()
}

fn pair(folders: &[Folder]) -> Option<(PathBuf, PathBuf)> {
    let work = folders.iter().find(|folder| workspace::is_inside(&folder.path))?;
    let project = folders.iter().find(|folder| !workspace::is_inside(&folder.path))?;
    Some((PathBuf::from(&project.path), PathBuf::from(&work.path)))
}

pub fn run(name: &str, arguments: &Value, folders: &[Folder]) -> Option<String> {
    if name != NAME {
        return None;
    }
    let Some((project, work)) = pair(folders) else {
        return Some("Error: this chat has no test environment next to a project folder".to_string());
    };
    let path = arguments["path"].as_str().unwrap_or_default().trim().trim_start_matches("./");
    Some(match sync(&project, &work, path) {
        Ok(report) => report,
        Err(e) => format!("Error: {e}"),
    })
}

fn relative(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if path.is_absolute() || path.components().any(|part| matches!(part, Component::ParentDir)) {
        return Err("path must be relative to the project and must not contain ..".to_string());
    }
    Ok(path.to_path_buf())
}

pub fn sync(project: &Path, work: &Path, path: &str) -> Result<String, String> {
    let part = relative(path)?;
    let source = project.join(&part);
    let meta = fs::symlink_metadata(&source).map_err(|_| format!("{} does not exist in the project", part.display()))?;
    let files: Vec<PathBuf> = if meta.is_file() {
        vec![part.clone()]
    } else if meta.is_dir() {
        let mut found = Vec::new();
        workspace::list_files(project, &source, &mut found);
        found
    } else {
        return Err(format!("{} is not a normal file or folder", part.display()));
    };
    let mut changed = Vec::new();
    let mut same = 0;
    for file in &files {
        let from = project.join(file);
        let to = work.join(file);
        let old = fs::read(&to).ok();
        let new = fs::read(&from).map_err(|e| format!("{}: {e}", file.display()))?;
        if old.as_deref() == Some(new.as_slice()) {
            same += 1;
            continue;
        }
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(&to, &new).map_err(|e| format!("{}: {e}", file.display()))?;
        changed.push(file.display().to_string());
    }
    let mut report = format!("{} file(s) copied from the project, {same} already the same.", changed.len());
    for file in changed.iter().take(LISTED) {
        report.push_str(&format!("\n- {file}"));
    }
    if changed.len() > LISTED {
        report.push_str(&format!("\n… and {} more", changed.len() - LISTED));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_only_different_files() {
        let root = std::env::temp_dir().join(format!("dlc-sync-{}", std::process::id()));
        let project = root.join("project");
        let work = root.join("work");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(project.join("target")).unwrap();
        fs::create_dir_all(work.join("src")).unwrap();
        fs::write(project.join("src/a.rs"), "new").unwrap();
        fs::write(project.join("src/b.rs"), "same").unwrap();
        fs::write(project.join("target/big"), "skip").unwrap();
        fs::write(work.join("src/a.rs"), "old").unwrap();
        fs::write(work.join("src/b.rs"), "same").unwrap();
        let report = sync(&project, &work, "").unwrap();
        assert!(report.starts_with("1 file(s) copied"));
        assert_eq!(fs::read_to_string(work.join("src/a.rs")).unwrap(), "new");
        assert!(!work.join("target/big").exists());
        assert!(sync(&project, &work, "../x").is_err());
        assert!(sync(&project, &work, "src/b.rs").unwrap().starts_with("0 file(s)"));
        fs::remove_dir_all(root).unwrap();
    }
}
