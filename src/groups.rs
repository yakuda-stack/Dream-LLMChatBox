use crate::sandbox::Folder;
use crate::storage::{self, ChatFile};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const OVERVIEW: &str = "overview.md";
const MAX_OVERVIEW_CHARS: usize = 3000;
const MAX_MEMORY_FILE: u64 = 2_000_000;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Group {
    #[serde(skip)]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub folder: Option<Folder>,
    #[serde(default)]
    pub start_args: String,
    #[serde(default = "default_true")]
    pub use_start_args: bool,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default = "default_true")]
    pub use_global_start_args: bool,
    #[serde(default = "default_true")]
    pub memory_enabled: bool,
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub extra_start_args: Vec<String>,
    #[serde(default)]
    pub removed_start_args: Vec<String>,
    #[serde(default)]
    pub test_env: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs_folder: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
struct GroupExport {
    dream_llmchatbox_group: u32,
    group: Group,
    #[serde(default)]
    memory: BTreeMap<String, String>,
    #[serde(default)]
    chats: Vec<ChatFile>,
}

pub fn groups_dir() -> PathBuf {
    storage::data_dir().join("groups")
}

fn group_dir_in(base: &Path, id: &str) -> PathBuf {
    base.join(id)
}

pub fn memory_dir(id: &str) -> PathBuf {
    memory_dir_in(&groups_dir(), id)
}

fn memory_dir_in(base: &Path, id: &str) -> PathBuf {
    group_dir_in(base, id).join("memory")
}

pub fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().to_lowercase().chars() {
        let c = match c {
            'ä' => 'a',
            'ö' => 'o',
            'ü' => 'u',
            'ß' => 's',
            c if c.is_ascii_alphanumeric() => c,
            _ => '-',
        };
        if c == '-' && (slug.is_empty() || slug.ends_with('-')) {
            continue;
        }
        slug.push(c);
    }
    let slug = slug.trim_end_matches('-').to_string();
    if slug.is_empty() { "group".to_string() } else { slug }
}

fn unique_id(base: &Path, name: &str) -> String {
    let first = slug(name);
    let mut id = first.clone();
    let mut number = 2;
    while group_dir_in(base, &id).exists() {
        id = format!("{first}-{number}");
        number += 1;
    }
    id
}

pub fn list() -> Vec<Group> {
    list_in(&groups_dir())
}

fn list_in(base: &Path) -> Vec<Group> {
    let Ok(entries) = fs::read_dir(base) else {
        return Vec::new();
    };
    let mut groups: Vec<Group> = entries
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().to_string();
            load_in(base, &id)
        })
        .collect();
    groups.sort_by_key(|group| group.name.to_lowercase());
    groups
}

pub fn load(id: &str) -> Option<Group> {
    if id.is_empty() {
        return None;
    }
    load_in(&groups_dir(), id)
}

fn load_in(base: &Path, id: &str) -> Option<Group> {
    let text = fs::read_to_string(group_dir_in(base, id).join("group.json")).ok()?;
    let mut group: Group = serde_json::from_str(&text).ok()?;
    group.id = id.to_string();
    Some(group)
}

pub fn save(group: &Group) -> Result<(), String> {
    save_in(&groups_dir(), group)
}

fn save_in(base: &Path, group: &Group) -> Result<(), String> {
    let dir = group_dir_in(base, &group.id);
    fs::create_dir_all(dir.join("memory")).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(group).map_err(|e| e.to_string())?;
    fs::write(dir.join("group.json"), json).map_err(|e| e.to_string())
}

pub fn create(name: &str) -> Result<Group, String> {
    create_in(&groups_dir(), name)
}

fn create_in(base: &Path, name: &str) -> Result<Group, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::i18n::tr("The group needs a name"));
    }
    let group = Group {
        id: unique_id(base, name),
        name: name.to_string(),
        use_start_args: true,
        use_global_start_args: true,
        memory_enabled: true,
        ..Default::default()
    };
    save_in(base, &group)?;
    let overview = memory_dir_in(base, &group.id).join(OVERVIEW);
    if !overview.exists() {
        let text = format!(
            "# {name}\n\nShort overview of this group. The model reads this file in every chat of the group.\n"
        );
        fs::write(overview, text).map_err(|e| e.to_string())?;
    }
    Ok(group)
}

pub fn delete(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err(crate::i18n::tr("No group"));
    }
    let dir = group_dir_in(&groups_dir(), id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    crate::workspace::delete_group(id);
    storage::ungroup_all(id)
}

pub fn overview(id: &str) -> String {
    let text = fs::read_to_string(memory_dir(id).join(OVERVIEW)).unwrap_or_default();
    if text.chars().count() > MAX_OVERVIEW_CHARS {
        let short: String = text.chars().take(MAX_OVERVIEW_CHARS).collect();
        format!("{short}\n[overview cut – keep {OVERVIEW} short]")
    } else {
        text
    }
}

pub fn system_note(group: &Group) -> String {
    let mut note = format!("This chat belongs to the group \"{}\".", group.name);
    if group.memory_enabled {
        note.push_str(&format!(
            " The group has its own memory: Markdown files that you can list, search, read and write with the memory tools. Read the memory before you answer questions about this group's topic. Save important lasting facts, decisions and progress there, and keep {OVERVIEW} as a short summary of the group."
        ));
        let overview = overview(&group.id);
        if !overview.trim().is_empty() {
            note.push_str(&format!("\n\nContent of {OVERVIEW}:\n{}", overview.trim()));
        }
    }
    if let Some(docs) = &group.docs_folder {
        note.push_str(&format!(
            "\n\nThe group has a searchable document folder ({docs}). Use the tool search_documents to find facts in these documents before you answer questions about them, and name the file you took them from."
        ));
    }
    if group.use_start_args {
        let lines: Vec<&str> = group
            .start_args
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect();
        if !lines.is_empty() {
            note.push_str("\n\nRules for this group:\n");
            for line in lines {
                note.push_str(&format!("- {line}\n"));
            }
        }
    }
    note
}

fn read_memory(dir: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return files;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_text = path
            .extension()
            .is_some_and(|ext| ext == "md" || ext == "txt");
        let small = entry.metadata().is_ok_and(|meta| meta.is_file() && meta.len() <= MAX_MEMORY_FILE);
        if is_text && small && let Ok(text) = fs::read_to_string(&path) {
            files.insert(entry.file_name().to_string_lossy().to_string(), text);
        }
    }
    files
}

pub fn export(id: &str, with_chats: bool) -> Result<String, String> {
    let group = load(id).ok_or(crate::i18n::tr("Group not found"))?;
    let chats = if with_chats {
        storage::group_chat_files(id)
            .into_iter()
            .map(|mut chat| {
                chat.folders = String::new();
                chat.group = String::new();
                chat
            })
            .collect()
    } else {
        Vec::new()
    };
    export_text(&group, read_memory(&memory_dir(id)), chats)
}

fn export_text(group: &Group, memory: BTreeMap<String, String>, chats: Vec<ChatFile>) -> Result<String, String> {
    let group = Group {
        folder: None,
        docs_folder: None,
        collapsed: false,
        ..group.clone()
    };
    let memory = if group.memory_enabled { memory } else { BTreeMap::new() };
    serde_json::to_string_pretty(&GroupExport {
        dream_llmchatbox_group: 1,
        group,
        memory,
        chats,
    })
    .map_err(|e| e.to_string())
}

pub fn safe_file_name(name: &str) -> Option<String> {
    let name: String = name
        .trim()
        .chars()
        .filter(|c| c.is_alphanumeric() || " -_.".contains(*c))
        .collect();
    let name = name.trim().trim_start_matches('.').trim().to_string();
    if name.is_empty() {
        return None;
    }
    let name: String = name.chars().take(80).collect();
    Some(if name.ends_with(".md") || name.ends_with(".txt") {
        name
    } else {
        format!("{name}.md")
    })
}

pub struct Imported {
    pub group: Group,
    pub chats: usize,
}

pub fn import(text: &str) -> Result<Imported, String> {
    let (group, chats) = import_in(&groups_dir(), text)?;
    let count = chats.len();
    for mut chat in chats {
        chat.id = storage::new_id();
        chat.group = group.id.clone();
        chat.folders = String::new();
        storage::save_chat(&chat)?;
    }
    Ok(Imported { group, chats: count })
}

fn import_in(base: &Path, text: &str) -> Result<(Group, Vec<ChatFile>), String> {
    let file: GroupExport =
        serde_json::from_str(text).map_err(|_| crate::i18n::tr("This is not a Dream-LLMChatBox group file"))?;
    let taken: Vec<String> = list_in(base).into_iter().map(|group| group.name).collect();
    let mut name = file.group.name.trim().to_string();
    let mut number = 2;
    while taken.contains(&name) {
        name = format!("{} ({number})", file.group.name.trim());
        number += 1;
    }
    let mut group = create_in(base, &name)?;
    group.start_args = file.group.start_args.clone();
    group.use_start_args = file.group.start_args.trim().is_empty();
    group.purpose = file.group.purpose.clone();
    save_in(base, &group)?;
    let memory = memory_dir_in(base, &group.id);
    for (name, content) in file.memory {
        if let Some(name) = safe_file_name(&name) {
            fs::write(memory.join(name), content).map_err(|e| e.to_string())?;
        }
    }
    Ok((group, file.chats))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dlc-groups-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("yakuda-connect"), "yakuda-connect");
        assert_eq!(slug("  Mein Projekt: Über!  "), "mein-projekt-uber");
        assert_eq!(slug("///"), "group");
        assert_eq!(safe_file_name("../../etc/passwd").as_deref(), Some("etcpasswd.md"));
        assert_eq!(safe_file_name("Ideen.md").as_deref(), Some("Ideen.md"));
        assert_eq!(safe_file_name(" .. "), None);
    }

    #[test]
    fn notes_follow_switches() {
        let mut group = Group {
            id: "x".into(),
            name: "X".into(),
            start_args: "Rule one".into(),
            use_start_args: true,
            memory_enabled: true,
            ..Default::default()
        };
        assert!(system_note(&group).contains("memory tools"));
        assert!(system_note(&group).contains("- Rule one"));
        group.memory_enabled = false;
        group.use_start_args = false;
        assert_eq!(system_note(&group), "This chat belongs to the group \"X\".");
    }

    #[test]
    fn create_list_export_import() {
        let dir = base("a");
        let mut group = create_in(&dir, "Yakuda Connect").unwrap();
        assert_eq!(group.id, "yakuda-connect");
        assert!(memory_dir_in(&dir, &group.id).join(OVERVIEW).exists());
        let second = create_in(&dir, "yakuda connect").unwrap();
        assert_eq!(second.id, "yakuda-connect-2");
        group.start_args = "Answer in German".into();
        group.folder = Some(Folder {
            path: "/home/x/project".into(),
            writable: true,
        });
        save_in(&dir, &group).unwrap();
        assert_eq!(list_in(&dir).len(), 2);

        let mut memory = read_memory(&memory_dir_in(&dir, &group.id));
        memory.insert("../evil.sh".into(), "x".into());
        let text = export_text(&group, memory, Vec::new()).unwrap();
        assert!(!text.contains("/home/x/project"));

        let other = base("b");
        let (imported, chats) = import_in(&other, &text).unwrap();
        assert_eq!(imported.name, "Yakuda Connect");
        let (again, _) = import_in(&other, &text).unwrap();
        assert_eq!(again.name, "Yakuda Connect (2)");
        assert!(!imported.use_start_args);
        assert!(imported.folder.is_none());
        assert!(chats.is_empty());
        let files = read_memory(&memory_dir_in(&other, &imported.id));
        assert!(files.contains_key(OVERVIEW));
        assert!(files.contains_key("evil.sh.md"));
        assert!(import_in(&other, "{}").is_err());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&other);
    }
}
