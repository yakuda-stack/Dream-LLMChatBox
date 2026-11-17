use crate::groups;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

const MAX_WRITE: usize = 100_000;
const MAX_READ_CHARS: usize = 12_000;
const MAX_HITS: usize = 30;

pub const NAMES: [&str; 4] = ["memory_list", "memory_search", "memory_read", "memory_write"];

pub fn definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "memory_list",
                "description": "List the notes in this group's memory with their size and first line.",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "memory_search",
                "description": "Search all notes in this group's memory for words. Returns matching lines with the note name.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Words to look for" }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "memory_read",
                "description": "Read one note from this group's memory.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Note name, e.g. overview.md or decisions.md" }
                    },
                    "required": ["name"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "memory_write",
                "description": "Create or change a note in this group's memory (Markdown, [[links]] to other notes are fine). mode \"append\" adds to the end, \"replace\" writes the whole note new. Keep overview.md short.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "description": "Note name, e.g. decisions.md" },
                        "content": { "type": "string", "description": "Markdown text" },
                        "mode": { "type": "string", "enum": ["append", "replace"], "description": "Default append" }
                    },
                    "required": ["name", "content"]
                }
            }
        }),
    ]
}

pub fn run(name: &str, arguments: &Value, group: &str) -> Option<String> {
    if !NAMES.contains(&name) {
        return None;
    }
    match groups::load(group) {
        None => return Some("Error: this chat belongs to no group, so there is no memory".to_string()),
        Some(found) if !found.memory_enabled => {
            return Some("Error: the memory of this group is switched off".to_string());
        }
        Some(_) => {}
    }
    let dir = groups::memory_dir(group);
    let text = |key: &str| arguments[key].as_str().unwrap_or_default().to_string();
    let result = match name {
        "memory_list" => list(&dir),
        "memory_search" => search(&dir, &text("query")),
        "memory_read" => read(&dir, &text("name")),
        _ => write(&dir, &text("name"), &text("content"), &text("mode")),
    };
    Some(result.unwrap_or_else(|e| format!("Error: {e}")))
}

fn notes(dir: &Path) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut notes: Vec<(String, String)> = entries
        .flatten()
        .filter(|entry| {
            entry.path().extension().is_some_and(|ext| ext == "md" || ext == "txt")
        })
        .filter_map(|entry| {
            let text = fs::read_to_string(entry.path()).ok()?;
            Some((entry.file_name().to_string_lossy().to_string(), text))
        })
        .collect();
    notes.sort_by(|a, b| a.0.cmp(&b.0));
    notes
}

fn list(dir: &Path) -> Result<String, String> {
    let notes = notes(dir);
    if notes.is_empty() {
        return Ok("The memory is empty.".to_string());
    }
    let mut output = format!("{} note(s):\n", notes.len());
    for (name, text) in notes {
        let first = text
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default()
            .trim_start_matches('#')
            .trim();
        let first: String = first.chars().take(80).collect();
        output.push_str(&format!("- {name} ({} lines) – {first}\n", text.lines().count()));
    }
    Ok(output.trim_end().to_string())
}

fn search(dir: &Path, query: &str) -> Result<String, String> {
    let words: Vec<String> = query
        .split_whitespace()
        .map(|word| word.to_lowercase())
        .collect();
    if words.is_empty() {
        return Err("empty search".to_string());
    }
    let mut hits = Vec::new();
    for (name, text) in notes(dir) {
        for (number, line) in text.lines().enumerate() {
            let lower = line.to_lowercase();
            if words.iter().any(|word| lower.contains(word.as_str())) {
                let line: String = line.trim().chars().take(200).collect();
                hits.push(format!("{name}:{}: {line}", number + 1));
            }
        }
    }
    if hits.is_empty() {
        return Ok(format!("Nothing found for \"{query}\"."));
    }
    let total = hits.len();
    hits.truncate(MAX_HITS);
    let mut output = hits.join("\n");
    if total > MAX_HITS {
        output.push_str(&format!("\n[{} more hits, search more exactly]", total - MAX_HITS));
    }
    Ok(output)
}

fn read(dir: &Path, name: &str) -> Result<String, String> {
    let file = groups::safe_file_name(name).ok_or("invalid note name")?;
    let text = fs::read_to_string(dir.join(&file)).map_err(|_| format!("{file} does not exist – use memory_list"))?;
    if text.chars().count() > MAX_READ_CHARS {
        let short: String = text.chars().take(MAX_READ_CHARS).collect();
        Ok(format!("{file}:\n{short}\n[note cut, use memory_search for the rest]"))
    } else {
        Ok(format!("{file}:\n{text}"))
    }
}

fn write(dir: &Path, name: &str, content: &str, mode: &str) -> Result<String, String> {
    let file = groups::safe_file_name(name).ok_or("invalid note name")?;
    if content.trim().is_empty() {
        return Err("nothing to write".to_string());
    }
    if content.len() > MAX_WRITE {
        return Err("content too big (max. 100 KB)".to_string());
    }
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(&file);
    let existed = path.exists();
    let new_text = if mode == "replace" || !existed {
        content.to_string()
    } else {
        let old = fs::read_to_string(&path).unwrap_or_default();
        format!("{}\n\n{}", old.trim_end(), content.trim())
    };
    fs::write(&path, &new_text).map_err(|e| e.to_string())?;
    let action = match (existed, mode == "replace") {
        (false, _) => "Created",
        (true, true) => "Replaced",
        (true, false) => "Added to",
    };
    Ok(format!("{action} {file} ({} lines)", new_text.lines().count()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_work() {
        let dir = std::env::temp_dir().join(format!("dlc-memory-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(list(&dir).unwrap(), "The memory is empty.");
        assert_eq!(write(&dir, "Entscheidungen", "# Entscheidungen\nRust statt Python", "").unwrap(), "Created Entscheidungen.md (2 lines)");
        assert!(write(&dir, "Entscheidungen.md", "Slint für die UI", "append").unwrap().starts_with("Added to"));
        assert!(read(&dir, "Entscheidungen").unwrap().contains("Rust statt Python\n\nSlint"));
        assert!(search(&dir, "slint").unwrap().contains("Entscheidungen.md:4: Slint für die UI"));
        assert!(list(&dir).unwrap().contains("- Entscheidungen.md (4 lines) – Entscheidungen"));
        assert!(write(&dir, "../../x", "y", "").unwrap().starts_with("Created x.md"));
        assert!(!dir.parent().unwrap().join("x.md").exists());
        assert!(write(&dir, "a", " ", "").is_err());
        assert!(read(&dir, "missing").is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
