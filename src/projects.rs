use crate::coding;
use crate::groups;
use std::fs;
use std::path::{Path, PathBuf};

pub const INDEX_FILE: &str = "architecture-index.md";
const INDEX_CHARS: usize = 2500;
const GLOBAL_TEMPLATE: &str = "# Global memory\n\nShort facts every chat may look up with the tool list_projects.\nOne project per line: `- Name: /path/to/folder – short description`\n\n";

pub struct Project {
    pub name: String,
    pub folder: PathBuf,
    pub note: String,
}

pub fn global_file() -> PathBuf {
    crate::storage::data_dir().join("global.md")
}

pub fn ensure_global() -> PathBuf {
    let path = global_file();
    if !path.exists() {
        let _ = fs::write(&path, GLOBAL_TEMPLATE);
    }
    path
}

pub fn parse_global(text: &str) -> Vec<Project> {
    text.lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("- ")?;
            let (name, rest) = rest.split_once(':')?;
            let rest = rest.trim();
            let (path, note) = match rest.split_once(" – ").or_else(|| rest.split_once(" - ")) {
                Some((path, note)) => (path.trim(), note.trim()),
                None => (rest, ""),
            };
            let path = path.trim_matches('`');
            if !(path.starts_with('/') || path.starts_with("~/")) {
                return None;
            }
            let folder = match path.strip_prefix("~/") {
                Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
                None => PathBuf::from(path),
            };
            Some(Project {
                name: name.trim().trim_matches('`').to_string(),
                folder,
                note: note.to_string(),
            })
        })
        .collect()
}

pub fn all() -> Vec<Project> {
    let mut list: Vec<Project> = groups::list()
        .into_iter()
        .filter_map(|group| {
            let folder = group.folder?;
            Some(Project {
                name: group.name,
                folder: PathBuf::from(folder.path),
                note: if group.purpose.is_empty() {
                    "group".to_string()
                } else {
                    format!("group, {}", group.purpose)
                },
            })
        })
        .collect();
    let global = fs::read_to_string(global_file()).unwrap_or_default();
    for project in parse_global(&global) {
        if !list.iter().any(|known| known.folder == project.folder) {
            list.push(project);
        }
    }
    list
}

pub fn listing() -> String {
    let projects = all();
    if projects.is_empty() {
        return "No projects known yet. Groups with a folder and lines in the global memory file show up here.".to_string();
    }
    let mut out = String::from("Known projects (you do NOT have access yet – use request_project_access first):\n");
    for project in projects {
        let architecture = coding::find_architecture(&project.folder)
            .map(|path| format!(", has {}", coding::relative(&project.folder, &path)))
            .unwrap_or_default();
        let note = if project.note.is_empty() { String::new() } else { format!(" – {}", project.note) };
        out.push_str(&format!("- {}: {}{note}{architecture}\n", project.name, project.folder.display()));
    }
    out
}

pub fn find(name: &str) -> Option<Project> {
    let wanted = name.trim().to_lowercase();
    let projects = all();
    let exact = projects.iter().position(|project| project.name.to_lowercase() == wanted);
    let index = exact.or_else(|| projects.iter().position(|project| project.name.to_lowercase().contains(&wanted)))?;
    projects.into_iter().nth(index)
}

pub fn index_of(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            out.push_str(trimmed);
            out.push('\n');
        } else if let Some(row) = trimmed.strip_prefix('|') {
            let cells: Vec<&str> = row.split('|').map(str::trim).collect();
            let first = cells.first().copied().unwrap_or_default();
            if first.is_empty() || first.starts_with('-') || first.eq_ignore_ascii_case("area") || first.eq_ignore_ascii_case("what") {
                continue;
            }
            let files = cells.get(1).copied().unwrap_or_default();
            if files.contains('`') {
                out.push_str(&format!("- {first}: {files}\n"));
            }
        }
        if out.len() > INDEX_CHARS {
            out.push_str("…\n");
            break;
        }
    }
    out
}

pub fn architecture_index(root: &Path) -> Option<(String, String)> {
    let path = coding::find_architecture(root)?;
    let text = fs::read_to_string(&path).ok()?;
    let index = index_of(&text);
    (!index.trim().is_empty()).then(|| (coding::relative(root, &path), index))
}

pub fn refresh_group_index(group: &groups::Group) {
    let Some(folder) = group.folder.as_ref().filter(|_| group.memory_enabled) else {
        return;
    };
    let target = groups::memory_dir(&group.id).join(INDEX_FILE);
    let Some((name, index)) = architecture_index(Path::new(&folder.path)) else {
        let _ = fs::remove_file(target);
        return;
    };
    let content = format!(
        "# Architecture index (made automatically – do not edit)\n\nFrom {} in {}. Open the full file only for details.\n\n{index}",
        name, folder.path
    );
    if fs::read_to_string(&target).ok().as_deref() != Some(content.as_str()) {
        let _ = fs::create_dir_all(groups::memory_dir(&group.id));
        let _ = fs::write(target, content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_lines_and_index() {
        let projects = parse_global("# G\n- Viewshot: /home/y/ViewShot – VR photos\n- Bad line\n- Home: ~/notes\n");
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name, "Viewshot");
        assert_eq!(projects[0].note, "VR photos");
        assert!(projects[1].folder.ends_with("notes"));
        let index = index_of("# Arch\n## Modules\n| Area | Files | Job |\n|---|---|---|\n| Chat flow | `chat.rs` | send |\n| Notes | none | x |\n");
        assert_eq!(index, "# Arch\n## Modules\n- Chat flow: `chat.rs`\n");
    }
}
