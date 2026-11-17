use crate::sandbox::Folder;
use crate::settings;
use std::fs;
use std::path::{Path, PathBuf};

const SKIP: [&str; 8] = [".git", "target", "node_modules", ".venv", "venv", "__pycache__", "build", "dist"];
const NO_GROUP: &str = "_no-group";
const MAX_FILES: usize = 20_000;
const MARKER: &str = ".dream-work";

pub fn base() -> PathBuf {
    let configured = settings::load().work_dir;
    let configured = configured.trim();
    if !configured.is_empty() {
        return match configured.strip_prefix("~/") {
            Some(rest) => home().join(rest),
            None => PathBuf::from(configured),
        };
    }
    home().join("DreamWork")
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn safe(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
        .collect();
    let cleaned = cleaned.trim_matches('-').to_string();
    if cleaned.is_empty() { "chat".to_string() } else { cleaned }
}

pub fn group_dir_in(base: &Path, group: &str) -> PathBuf {
    base.join(if group.is_empty() { NO_GROUP.to_string() } else { safe(group) })
}

pub fn chat_dir_in(base: &Path, group: &str, chat: &str) -> PathBuf {
    group_dir_in(base, group).join(safe(chat))
}

pub fn chat_dir(group: &str, chat: &str) -> PathBuf {
    chat_dir_in(&base(), group, chat)
}

pub fn delete_group(group: &str) {
    if group.is_empty() {
        return;
    }
    let _ = fs::remove_dir_all(group_dir_in(&base(), group));
}

pub fn delete_chat(group: &str, chat: &str) {
    let _ = fs::remove_dir_all(chat_dir(group, chat));
}

pub fn is_inside(path: &str) -> bool {
    let base = base();
    let base = base.canonicalize().unwrap_or(base);
    Path::new(path).starts_with(&base)
}

pub fn has_copy(dir: &Path) -> bool {
    dir.join(MARKER).exists()
}

pub fn test_env_note(dir: &Path, copied: bool) -> String {
    let copy = if copied {
        " It holds a copy of the user's project (without .git, target, node_modules)."
    } else {
        ""
    };
    format!(
        "Test environment: {} is your own folder for this chat.{copy} You may create, change, build and run anything there; terminal commands run in it.{} Never change the user's original project folder. If a file here is missing or differs from the project, call sync_from_project to copy it over. After changing code, check it here (for example cargo check, a build or the tests) and fix errors before you show the result. When you are done, tell the user which files you changed – the user takes them over with \"Apply to project\".",
        dir.display(),
        crate::container::note().unwrap_or_default()
    )
}

pub fn needs_work_folder(shared: &[Folder]) -> bool {
    !shared.is_empty() && !shared.iter().any(|folder| folder.writable || is_inside(&folder.path))
}

pub fn copy_project(from: &Path, to: &Path) -> Result<usize, String> {
    fs::create_dir_all(to).map_err(|e| e.to_string())?;
    let mut count = 0;
    copy_dir(from, to, &mut count)?;
    fs::write(to.join(MARKER), "copied").map_err(|e| e.to_string())?;
    Ok(count)
}

fn copy_dir(from: &Path, to: &Path, count: &mut usize) -> Result<(), String> {
    for entry in fs::read_dir(from).map_err(|e| e.to_string())?.flatten() {
        let name = entry.file_name();
        if SKIP.iter().any(|skip| name == *skip) {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        let target = to.join(&name);
        if kind.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            copy_dir(&entry.path(), &target, count)?;
        } else if kind.is_file() {
            *count += 1;
            if *count > MAX_FILES {
                return Err(crate::i18n::trf("more than {} files, copy stopped", &[&MAX_FILES]));
            }
            fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub struct Change {
    pub path: String,
    pub new: bool,
    pub diff: String,
}

pub fn changes(project: &Path, work: &Path) -> Vec<Change> {
    let mut files = Vec::new();
    list_files(work, work, &mut files);
    files.sort();
    let copied_at = fs::metadata(work.join(MARKER)).and_then(|meta| meta.modified()).ok();
    files
        .into_iter()
        .filter(|relative| relative.as_os_str() != MARKER)
        .filter(|relative| {
            let touched = fs::metadata(work.join(relative)).and_then(|meta| meta.modified()).ok();
            match (copied_at, touched) {
                (Some(copied), Some(touched)) => touched > copied || !project.join(relative).exists(),
                _ => true,
            }
        })
        .filter_map(|relative| {
            let after = fs::read(work.join(&relative)).ok()?;
            let before = fs::read(project.join(&relative)).ok();
            if before.as_deref() == Some(after.as_slice()) {
                return None;
            }
            let before_text = before.as_deref().map(String::from_utf8_lossy).unwrap_or_default();
            let after_text = String::from_utf8_lossy(&after);
            let diff = if after.contains(&0) {
                crate::i18n::tr("(binary file)")
            } else {
                similar::TextDiff::from_lines(before_text.as_ref(), after_text.as_ref())
                    .unified_diff()
                    .context_radius(2)
                    .to_string()
            };
            Some(Change {
                path: relative.to_string_lossy().to_string(),
                new: before.is_none(),
                diff,
            })
        })
        .collect()
}

pub fn list_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if SKIP.iter().any(|skip| name == *skip) {
            continue;
        }
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            list_files(root, &entry.path(), out);
        } else if kind.is_file()
            && let Ok(relative) = entry.path().strip_prefix(root)
        {
            out.push(relative.to_path_buf());
        }
    }
}

pub fn apply(project: &Path, work: &Path, changes: &[Change]) -> Result<usize, String> {
    for change in changes {
        let target = project.join(&change.path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(work.join(&change.path), &target).map_err(|e| format!("{}: {e}", change.path))?;
    }
    Ok(changes.len())
}

pub fn report(changes: &[Change]) -> String {
    changes
        .iter()
        .map(|change| {
            format!(
                "{} {}\n{}",
                if change.new { crate::i18n::tr("＋ NEW") } else { crate::i18n::tr("✎ CHANGED") },
                change.path,
                change.diff.trim_end()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn system_note(dir: &Path, copied: bool) -> String {
    let copy = if copied {
        " It holds a fresh copy of the shared project (without .git, target, node_modules)."
    } else {
        ""
    };
    format!(
        "Your own work folder for this chat is {} and you may write there.{copy} The shared project folder is read-only: never try to change it. Make and test changes in the work folder, then show the user the changed code so the user can apply it.",
        dir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_and_copy() {
        let root = std::env::temp_dir().join(format!("dlc-work-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        assert_eq!(chat_dir_in(&root, "My Group!", "abc 1"), root.join("My-Group").join("abc-1"));
        assert_eq!(group_dir_in(&root, ""), root.join("_no-group"));
        let project = root.join("project");
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(project.join("target")).unwrap();
        fs::write(project.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(project.join("target/big"), "x").unwrap();
        let copy = root.join("copy");
        assert_eq!(copy_project(&project, &copy).unwrap(), 1);
        assert!(changes(&project, &copy).is_empty());
        assert!(copy.join("src/main.rs").exists());
        assert!(!copy.join("target").exists());
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(copy.join("src/main.rs"), "fn main() { new(); }").unwrap();
        fs::write(copy.join("added.txt"), "hi").unwrap();
        let found = changes(&project, &copy);
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|c| c.path == "added.txt" && c.new));
        assert!(report(&found).contains("+fn main() { new(); }"));
        apply(&project, &copy, &found).unwrap();
        assert!(changes(&project, &copy).is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
