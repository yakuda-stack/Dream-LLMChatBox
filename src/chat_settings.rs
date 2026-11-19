use crate::i18n::tr;
use crate::sandbox::{self, Folder};
use crate::{AppWindow, ChatSettings, bwrap, folders, groups, storage, workspace};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::fs;
use std::process::{Command, Stdio};

thread_local! {
    static GROUP: RefCell<String> = const { RefCell::new(String::new()) };
}

fn folder_lines(list: &[Folder]) -> String {
    if list.is_empty() {
        return tr("No folders shared");
    }
    list.iter()
        .map(|folder| {
            let access = if folder.writable { tr("read + write") } else { tr("read only") };
            let name = if workspace::is_inside(&folder.path) {
                format!("🧪 {} – ", tr("Test environment"))
            } else {
                String::new()
            };
            format!("{name}{} ({access})", folder.path)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn show(ui: &AppWindow, id: &str) {
    let Ok(chat) = storage::load_chat(id) else { return };
    let settings = ui.global::<ChatSettings>();
    let group = groups::load(&chat.group);
    let shared = if ui.get_current_chat_id().as_str() == id {
        folders::current(ui)
    } else {
        sandbox::from_json(&chat.folders)
    };
    let (memory, has_memory) = match &group {
        Some(group) if group.memory_enabled => (groups::memory_dir(&group.id).display().to_string(), true),
        Some(_) => (tr("Switched off in the group settings"), false),
        None => (tr("Only chats in a group have a memory"), false),
    };
    GROUP.with(|current| *current.borrow_mut() = chat.group.clone());
    settings.set_chat_id(id.into());
    settings.set_title(chat.title.into());
    settings.set_group(group.map(|group| group.name).unwrap_or_else(|| tr("No group")).into());
    settings.set_folders(folder_lines(&shared).into());
    settings.set_memory(memory.into());
    settings.set_has_memory(has_memory);
    settings.set_sandbox(bwrap::enabled(id, &chat.group));
    settings.set_sandbox_status(bwrap::status().into());
    settings.set_error("".into());
    settings.set_open(true);
}

fn save(ui: &AppWindow) {
    let settings = ui.global::<ChatSettings>();
    let id = settings.get_chat_id().to_string();
    let title = settings.get_title().trim().to_string();
    if title.is_empty() {
        settings.set_error(tr("The chat needs a name").into());
        return;
    }
    if let Err(e) = storage::rename_chat(&id, &title) {
        settings.set_error(e.into());
        return;
    }
    let group = GROUP.with(|current| current.borrow().clone());
    bwrap::set_chat(&id, &group, settings.get_sandbox());
    settings.set_open(false);
    crate::chat::refresh_chat_list(ui);
}

fn open_memory() {
    let group = GROUP.with(|current| current.borrow().clone());
    if group.is_empty() {
        return;
    }
    let dir = groups::memory_dir(&group);
    let _ = fs::create_dir_all(&dir);
    let _ = Command::new("xdg-open")
        .arg(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

pub fn setup(ui: &AppWindow) {
    let settings = ui.global::<ChatSettings>();

    let weak = ui.as_weak();
    settings.on_show(move |id| {
        if let Some(ui) = weak.upgrade() {
            show(&ui, &id);
        }
    });

    let weak = ui.as_weak();
    settings.on_save(move || {
        if let Some(ui) = weak.upgrade() {
            save(&ui);
        }
    });

    settings.on_open_memory(open_memory);
}
