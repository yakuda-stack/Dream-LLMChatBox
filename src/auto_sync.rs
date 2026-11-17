use crate::AppWindow;
use crate::folders;
use crate::i18n::{tr, trf};
use crate::project_sync;
use crate::workspace;
use slint::ComponentHandle;
use std::fs;
use std::path::{Path, PathBuf};

fn path() -> PathBuf {
    crate::storage::data_dir().join("auto_sync.json")
}

fn load_in(file: &Path) -> Vec<String> {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn set_in(file: &Path, chat: &str, on: bool) {
    let mut list = load_in(file);
    list.retain(|id| id != chat);
    if on {
        list.push(chat.to_string());
    }
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(&list) {
        let _ = fs::write(file, text);
    }
}

pub fn enabled(chat: &str) -> bool {
    !chat.is_empty() && load_in(&path()).iter().any(|id| id == chat)
}

pub fn setup(ui: &AppWindow) {
    let weak = ui.as_weak();
    ui.on_auto_sync_changed(move |on| {
        let Some(ui) = weak.upgrade() else { return };
        let chat = ui.get_current_chat_id().to_string();
        if !chat.is_empty() {
            set_in(&path(), &chat, on);
        }
    });
}

pub fn refresh(ui: &AppWindow) {
    let available = project_sync::available(&folders::current(ui));
    ui.set_sync_available(available);
    let chat = ui.get_current_chat_id().to_string();
    if !available {
        ui.set_auto_sync(false);
    } else if !chat.is_empty() {
        ui.set_auto_sync(enabled(&chat));
    }
}

pub fn before_send(ui: &AppWindow) -> Option<String> {
    if !ui.get_auto_sync() {
        return None;
    }
    let chat = ui.get_current_chat_id().to_string();
    if !chat.is_empty() {
        set_in(&path(), &chat, true);
    }
    let list = folders::current(ui);
    let work = list.iter().find(|folder| workspace::is_inside(&folder.path))?;
    let project = list.iter().find(|folder| !workspace::is_inside(&folder.path))?;
    match project_sync::sync(Path::new(&project.path), Path::new(&work.path), "") {
        Ok(report) => {
            let count = report.split_whitespace().next().unwrap_or("0").to_string();
            ui.set_status(trf("🔄 Test environment synced from the project: {} file(s) updated", &[&count]).into());
            Some(format!(
                "[The app just synced your test environment from the user's original project. {report} Your test environment now has the user's current files – always read files again before showing old code, and don't rely on what you read earlier.]"
            ))
        }
        Err(e) => {
            ui.set_status(format!("{}: {e}", tr("Sync failed")).into());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_chats() {
        let file = std::env::temp_dir().join(format!("dlc-auto-sync-{}.json", std::process::id()));
        set_in(&file, "a", true);
        set_in(&file, "b", true);
        set_in(&file, "a", false);
        assert_eq!(load_in(&file), vec!["b".to_string()]);
        let _ = fs::remove_file(file);
    }
}
