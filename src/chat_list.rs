use crate::i18n::{tr, trf};
use crate::chat;
use crate::groups;
use crate::groups_ui;
use crate::storage;
use crate::{AppWindow, ChatEntry, ChatList};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub const GROUP_PREFIX: &str = "group:";

thread_local! {
    static SELECTED: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
    static PINNED: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
}

pub fn entry(id: String, title: String) -> ChatEntry {
    ChatEntry {
        pinned: PINNED.with(|pinned| pinned.borrow().contains(&id)),
        selected: is_selected(&id),
        id: id.into(),
        title: title.into(),
    }
}

pub fn is_selected(key: &str) -> bool {
    SELECTED.with(|selected| selected.borrow().contains(key))
}

pub fn load_pinned() {
    let ids: BTreeSet<String> = storage::pinned_ids().into_iter().collect();
    PINNED.with(|pinned| *pinned.borrow_mut() = ids);
}

fn refresh(ui: &AppWindow) {
    load_pinned();
    let count = SELECTED.with(|selected| selected.borrow().len());
    let list = ui.global::<ChatList>();
    list.set_selection_count(count as i32);
    let all = groups::list();
    let ids: Vec<SharedString> = all.iter().map(|group| group.id.clone().into()).collect();
    let names: Vec<SharedString> = all.iter().map(|group| group.name.clone().into()).collect();
    list.set_group_ids(ModelRc::new(VecModel::from(ids)));
    list.set_group_names(ModelRc::new(VecModel::from(names)));
    search(ui);
    chat::refresh_chat_list(ui);
    groups_ui::refresh(ui);
}

pub fn search(ui: &AppWindow) {
    let list = ui.global::<ChatList>();
    let query = list.get_query().to_string();
    let found: Vec<ChatEntry> = if query.trim().is_empty() {
        Vec::new()
    } else {
        storage::search_chats(&query)
            .into_iter()
            .map(|(id, title)| entry(id, title))
            .collect()
    };
    list.set_results(ModelRc::new(VecModel::from(found)));
}

fn export(ui: &AppWindow, id: &str) {
    let Ok(chat) = storage::load_chat(id) else { return };
    let name: String = chat
        .title
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .collect();
    let Some(path) = rfd::FileDialog::new()
        .set_title(tr("Export chat"))
        .set_file_name(format!("{}.md", name.trim()))
        .add_filter("Markdown", &["md"])
        .save_file()
    else {
        return;
    };
    match fs::write(&path, to_markdown(&chat)) {
        Ok(()) => ui.set_status(trf("Chat exported to {}", &[&path.display()]).into()),
        Err(e) => ui.set_status(trf("Could not export: {}", &[&e]).into()),
    }
}

pub fn to_markdown(chat: &storage::ChatFile) -> String {
    let mut out = format!("# {}\n\n_Model: {}_\n", chat.title, chat.model);
    for message in &chat.messages {
        let text = crate::context_saver::strip_thinking(&crate::markdown::without_tool_blocks(&message.content));
        if text.is_empty() {
            continue;
        }
        let who = if message.role == "user" { "You" } else { "Assistant" };
        out.push_str(&format!("\n## {who}\n\n{text}\n"));
    }
    out
}

pub fn auto_title(ui: &AppWindow) {
    let id = ui.get_current_chat_id().to_string();
    let Ok(chat) = storage::load_chat(&id) else { return };
    let user = chat.messages.iter().filter(|m| m.role == "user").count();
    let Some(first) = chat.messages.iter().find(|m| m.role == "user") else { return };
    if user != 1 || chat.title != chat::default_title(&first.content) {
        return;
    }
    let question: String = first.content.chars().take(800).collect();
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let Ok(models) = crate::ollama::list_models() else { return };
        let Some(router) = crate::auto_switch::router_model(&models) else { return };
        let prompt = format!(
            "Write a short title (2 to 6 words, no quotes, no punctuation at the end) for a chat that starts with this message. Use the language of the message. Answer only with the title.\n\n{question}"
        );
        let Ok(answer) = crate::ollama::classify_with(&router.name, &prompt, crate::auto_switch::ROUTER_KEEP_ALIVE, 24) else { return };
        let title = clean_title(&answer);
        if title.is_empty() {
            return;
        }
        let _ = weak.upgrade_in_event_loop(move |ui| {
            if storage::rename_chat(&id, &title).is_ok() {
                refresh(&ui);
            }
        });
    });
}

pub fn clean_title(answer: &str) -> String {
    let line = answer
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let line = line.trim_matches(|c: char| c == '"' || c == '\'' || c == '*' || c == '#' || c == '.' || c.is_whitespace());
    let line = line.strip_prefix("Title:").or_else(|| line.strip_prefix("Titel:")).unwrap_or(line).trim();
    line.chars().take(60).collect()
}

pub fn setup(ui: &AppWindow) {
    let list = ui.global::<ChatList>();

    let weak = ui.as_weak();
    list.on_search(move |_| {
        if let Some(ui) = weak.upgrade() {
            search(&ui);
        }
    });

    let weak = ui.as_weak();
    list.on_export(move |id| {
        if let Some(ui) = weak.upgrade() {
            export(&ui, &id);
        }
    });

    let weak = ui.as_weak();
    list.on_select(move |key| {
        let Some(ui) = weak.upgrade() else { return };
        SELECTED.with(|selected| {
            let mut selected = selected.borrow_mut();
            if !selected.remove(key.as_str()) {
                selected.insert(key.to_string());
            }
        });
        refresh(&ui);
    });

    let weak = ui.as_weak();
    list.on_clear_selection(move || {
        let Some(ui) = weak.upgrade() else { return };
        SELECTED.with(|selected| selected.borrow_mut().clear());
        refresh(&ui);
    });

    let weak = ui.as_weak();
    list.on_toggle_pin(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        let pinned = PINNED.with(|pinned| pinned.borrow().contains(id.as_str()));
        if let Err(e) = storage::set_pinned(&id, !pinned) {
            ui.set_status(trf("Could not pin the chat: {}", &[&e]).into());
        }
        refresh(&ui);
    });

    let weak = ui.as_weak();
    list.on_move(move |id, delta| {
        let Some(ui) = weak.upgrade() else { return };
        if delta != 0
            && let Err(e) = storage::move_chat(&id, delta as i64)
        {
            ui.set_status(trf("Could not move the chat: {}", &[&e]).into());
        }
        refresh(&ui);
    });

    let weak = ui.as_weak();
    list.on_assign(move |id, group| {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_busy() {
            return;
        }
        if let Err(e) = storage::set_chat_group(&id, &group) {
            ui.set_status(trf("Could not move the chat: {}", &[&e]).into());
        }
        if ui.get_current_chat_id() == id {
            groups_ui::set_current(&ui, &group);
        }
        refresh(&ui);
    });

    let weak = ui.as_weak();
    list.on_merge(move |name| {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_busy() {
            ui.set_status(tr("Wait until the answer is finished.").into());
            return;
        }
        match merge(&name) {
            Ok((note, group)) => {
                SELECTED.with(|selected| selected.borrow_mut().clear());
                if storage::load_chat(&ui.get_current_chat_id()).is_ok_and(|chat| chat.group == group) {
                    groups_ui::set_current(&ui, &group);
                }
                ui.set_status(note.into());
            }
            Err(e) => ui.set_status(trf("Could not merge: {}", &[&e]).into()),
        }
        refresh(&ui);
    });

    refresh(ui);
}

fn merge(name: &str) -> Result<(String, String), String> {
    let selected: Vec<String> = SELECTED.with(|selected| selected.borrow().iter().cloned().collect());
    let mut group_ids: Vec<String> = Vec::new();
    let mut chat_ids: Vec<String> = Vec::new();
    for key in &selected {
        if let Some(group) = key.strip_prefix(GROUP_PREFIX) {
            push_unique(&mut group_ids, group);
            for (chat, _) in storage::list_group_chats(group) {
                push_unique(&mut chat_ids, &chat);
            }
        } else {
            push_unique(&mut chat_ids, key);
            if let Ok(chat) = storage::load_chat(key)
                && !chat.group.is_empty()
            {
                push_unique(&mut group_ids, &chat.group);
            }
        }
    }
    if selected.len() < 2 {
        return Err(tr("select at least two chats or groups (Ctrl + click)"));
    }
    let default_name = tr("Merged group");
    let name = if name.trim().is_empty() { default_name.as_str() } else { name.trim() };
    let target = groups::create(name)?;
    let sources: Vec<(String, std::path::PathBuf)> = group_ids
        .iter()
        .filter_map(|id| groups::load(id).map(|group| (group.name, groups::memory_dir(id))))
        .collect();
    merge_memories(&sources, &groups::memory_dir(&target.id), name)?;
    for chat in &chat_ids {
        storage::set_chat_group(chat, &target.id)?;
    }
    let note = trf(
        "New group “{}”: {} memories merged, {} chats moved.",
        &[&name, &sources.len(), &chat_ids.len()],
    );
    Ok((note, target.id))
}

fn push_unique(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|item| item == value) {
        list.push(value.to_string());
    }
}

pub fn merge_memories(sources: &[(String, std::path::PathBuf)], target: &Path, title: &str) -> Result<(), String> {
    fs::create_dir_all(target).map_err(|e| e.to_string())?;
    let names: Vec<&str> = sources.iter().map(|(name, _)| name.as_str()).collect();
    let mut overview = if names.is_empty() {
        format!("# {title}\n")
    } else {
        format!("# {title}\n\nMerged from: {}\n", names.join(", "))
    };
    for (name, dir) in sources {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let file = entry.file_name().to_string_lossy().to_string();
            if file == groups::OVERVIEW {
                let text = fs::read_to_string(&path).unwrap_or_default();
                overview.push_str(&format!("\n## From {name}\n\n{}\n", text.trim()));
                continue;
            }
            fs::copy(&path, free_name(target, &file)).map_err(|e| e.to_string())?;
        }
    }
    fs::write(target.join(groups::OVERVIEW), overview).map_err(|e| e.to_string())
}

fn free_name(dir: &Path, file: &str) -> std::path::PathBuf {
    let candidate = dir.join(file);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match file.rsplit_once('.') {
        Some((stem, ext)) => (stem.to_string(), format!(".{ext}")),
        None => (file.to_string(), String::new()),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|path| !path.exists())
        .unwrap_or(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_cleaned() {
        assert_eq!(clean_title("\"Rust Fehler beheben.\"\n"), "Rust Fehler beheben");
        assert_eq!(clean_title("Title: Kochen mit Reis"), "Kochen mit Reis");
    }

    #[test]
    fn merges_memory_folders() {
        let root = std::env::temp_dir().join(format!("dlc-merge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let a = root.join("a");
        let b = root.join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("overview.md"), "# A\nalpha").unwrap();
        fs::write(b.join("overview.md"), "# B\nbeta").unwrap();
        fs::write(a.join("notes.md"), "a notes").unwrap();
        fs::write(b.join("notes.md"), "b notes").unwrap();
        let target = root.join("t");
        merge_memories(&[("A".into(), a), ("B".into(), b)], &target, "AB").unwrap();
        let overview = fs::read_to_string(target.join("overview.md")).unwrap();
        assert!(overview.contains("## From A") && overview.contains("beta"));
        assert!(target.join("notes.md").exists() && target.join("notes (2).md").exists());
        let _ = fs::remove_dir_all(&root);
    }
}
