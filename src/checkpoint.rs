use crate::sandbox::{self, Folder};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const MANIFEST: &str = "manifest.json";

#[derive(Serialize, Deserialize, Clone)]
struct Entry {
    path: String,
    existed: bool,
    copy: String,
}

fn base() -> PathBuf {
    crate::storage::data_dir().join("checkpoints")
}

pub fn dir_in(base: &Path, chat: &str, answer: usize) -> PathBuf {
    base.join(chat).join(answer.to_string())
}

pub fn dir(chat: &str, answer: usize) -> PathBuf {
    dir_in(&base(), chat, answer)
}

fn read(dir: &Path) -> Vec<Entry> {
    fs::read_to_string(dir.join(MANIFEST))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(dir: &Path, folders: &[Folder], path: &str) {
    let Ok(target) = sandbox::resolve(folders, path) else { return };
    let mut entries = read(dir);
    let shown = target.path.to_string_lossy().to_string();
    if entries.iter().any(|entry| entry.path == shown) || fs::create_dir_all(dir).is_err() {
        return;
    }
    let existed = target.path.is_file();
    let copy = format!("{}.bak", entries.len());
    if existed && fs::copy(&target.path, dir.join(&copy)).is_err() {
        return;
    }
    entries.push(Entry { path: shown, existed, copy });
    if let Ok(text) = serde_json::to_string_pretty(&entries) {
        let _ = fs::write(dir.join(MANIFEST), text);
    }
}

pub fn shift(chat: &str, from: usize, by: usize) {
    let folder = base().join(chat);
    let Ok(entries) = fs::read_dir(&folder) else { return };
    let mut answers: Vec<usize> = entries
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
        .filter(|answer| *answer >= from)
        .collect();
    answers.sort_unstable_by(|a, b| b.cmp(a));
    for answer in answers {
        let _ = fs::rename(folder.join(answer.to_string()), folder.join((answer + by).to_string()));
    }
}

pub fn count(dir: &Path) -> usize {
    read(dir).len()
}

pub fn undo(dir: &Path) -> Result<usize, String> {
    let entries = read(dir);
    for entry in entries.iter().rev() {
        let target = Path::new(&entry.path);
        if entry.existed {
            fs::copy(dir.join(&entry.copy), target).map_err(|e| format!("{}: {e}", entry.path))?;
        } else if target.exists() {
            fs::remove_file(target).map_err(|e| format!("{}: {e}", entry.path))?;
        }
    }
    let _ = fs::remove_dir_all(dir);
    Ok(entries.len())
}

pub fn clear(chat: &str, answer: usize) {
    let _ = fs::remove_dir_all(dir(chat, answer));
}

pub fn delete_chat(chat: &str) {
    if !chat.is_empty() {
        let _ = fs::remove_dir_all(base().join(chat));
    }
}

pub fn paths(chat: &str) -> Vec<String> {
    if chat.is_empty() {
        return Vec::new();
    }
    let mut answers: Vec<(usize, PathBuf)> = fs::read_dir(base().join(chat))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| Some((entry.file_name().to_string_lossy().parse().ok()?, entry.path())))
                .collect()
        })
        .unwrap_or_default();
    answers.sort_by_key(|answer| std::cmp::Reverse(answer.0));
    let mut out: Vec<String> = Vec::new();
    for (_, dir) in answers {
        for entry in read(&dir).into_iter().rev() {
            if !out.contains(&entry.path) {
                out.push(entry.path);
            }
        }
    }
    out
}

pub fn latest(chat: &str, answers: usize) -> Option<(usize, usize)> {
    (0..answers)
        .rev()
        .map(|index| (index, count(&dir(chat, index))))
        .find(|(_, files)| *files > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_restores_and_removes() {
        let root = std::env::temp_dir().join(format!("dlc-checkpoint-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("a.txt"), "old").unwrap();
        let folders = vec![Folder { path: project.to_string_lossy().to_string(), writable: true }];
        let dir = dir_in(&root.join("cp"), "chat", 3);
        save(&dir, &folders, "a.txt");
        save(&dir, &folders, "new.txt");
        fs::write(project.join("a.txt"), "changed").unwrap();
        save(&dir, &folders, "a.txt");
        fs::write(project.join("a.txt"), "changed twice").unwrap();
        fs::write(project.join("new.txt"), "created").unwrap();
        assert_eq!(count(&dir), 2);
        assert_eq!(undo(&dir).unwrap(), 2);
        assert_eq!(fs::read_to_string(project.join("a.txt")).unwrap(), "old");
        assert!(!project.join("new.txt").exists());
        assert!(!dir.exists());
        let _ = fs::remove_dir_all(&root);
    }
}
