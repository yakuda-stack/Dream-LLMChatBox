use crate::i18n::{tr, trf};
use crate::folders;
use crate::groups::{self, Group};
use crate::sandbox::{self, Folder};
use crate::storage;
use crate::{AppWindow, ChatEntry, GroupArg, GroupEntry};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;

thread_local! {
    static CURRENT: RefCell<String> = const { RefCell::new(String::new()) };
    static EDITING: RefCell<String> = const { RefCell::new(String::new()) };
    static ASSIGN_NEW: RefCell<bool> = const { RefCell::new(false) };
}

pub fn setup(ui: &AppWindow) {
    refresh(ui);

    let weak = ui.as_weak();
    ui.on_group_toggle(move |id| {
        if let Some(mut group) = groups::load(&id) {
            group.collapsed = !group.collapsed;
            let _ = groups::save(&group);
        }
        if let Some(ui) = weak.upgrade() {
            refresh(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_chat_group_selected(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let id = if index <= 0 {
            String::new()
        } else {
            groups::list()
                .get(index as usize - 1)
                .map(|group| group.id.clone())
                .unwrap_or_default()
        };
        choose(&ui, &id);
    });

    let weak = ui.as_weak();
    ui.on_group_new_chat(move |id| {
        if let Some(ui) = weak.upgrade() {
            ui.invoke_new_chat();
            choose(&ui, &id);
        }
    });

    let weak = ui.as_weak();
    ui.on_group_create(move || {
        let Some(ui) = weak.upgrade() else { return };
        EDITING.with(|editing| editing.borrow_mut().clear());
        ASSIGN_NEW.with(|assign| *assign.borrow_mut() = ui.get_page() == 0 && ui.get_messages().row_count() == 0);
        ui.set_group_dialog_new(true);
        ui.set_group_dialog_name("".into());
        ui.set_group_dialog_folder("".into());
        ui.set_group_dialog_writable(false);
        ui.set_group_dialog_rules("".into());
        ui.set_group_dialog_use_rules(true);
        ui.set_group_dialog_use_global(true);
        set_extra_args(&ui, &[], &[]);
        ui.set_group_dialog_test_env(false);
        ui.set_group_dialog_docs("".into());
        ui.set_group_dialog_docs_status("".into());
        ui.set_group_dialog_memory(true);
        let assign = ASSIGN_NEW.with(|assign| *assign.borrow());
        ui.set_group_dialog_purpose(i32::from(assign && crate::coding_ui::current().is_coding()));
        ui.set_group_dialog_error("".into());
        ui.set_group_dialog_open(true);
    });

    let weak = ui.as_weak();
    ui.on_group_settings(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        let Some(group) = groups::load(&id) else { return };
        EDITING.with(|editing| *editing.borrow_mut() = group.id.clone());
        ASSIGN_NEW.with(|assign| *assign.borrow_mut() = false);
        ui.set_group_dialog_new(false);
        ui.set_group_dialog_name(group.name.into());
        let (path, writable) = group
            .folder
            .map(|folder| (folder.path, folder.writable))
            .unwrap_or_default();
        ui.set_group_dialog_folder(path.into());
        ui.set_group_dialog_writable(writable);
        ui.set_group_dialog_rules(group.start_args.into());
        ui.set_group_dialog_use_rules(group.use_start_args);
        ui.set_group_dialog_use_global(group.use_global_start_args);
        set_extra_args(&ui, &group.extra_start_args, &group.removed_start_args);
        ui.set_group_dialog_test_env(group.test_env);
        let docs = group.docs_folder.clone().unwrap_or_default();
        ui.set_group_dialog_docs_status(crate::rag_ui::status_text(&docs).into());
        ui.set_group_dialog_docs(docs.into());
        ui.set_group_dialog_memory(group.memory_enabled);
        ui.set_group_dialog_purpose(i32::from(group.purpose == crate::coding::CODING));
        ui.set_group_dialog_error("".into());
        ui.set_group_dialog_open(true);
    });

    let weak = ui.as_weak();
    ui.on_group_dialog_pick_docs(move || {
        let weak = weak.clone();
        thread::spawn(move || {
            let Some(path) = rfd::FileDialog::new().set_title(tr("Document folder of the group")).pick_folder() else {
                return;
            };
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let text = path.to_string_lossy().to_string();
                ui.set_group_dialog_docs_status(crate::rag_ui::status_text(&text).into());
                ui.set_group_dialog_docs(text.into());
            });
        });
    });

    let weak = ui.as_weak();
    ui.on_group_dialog_index_docs(move || {
        let Some(ui) = weak.upgrade() else { return };
        let docs = ui.get_group_dialog_docs().to_string();
        if !docs.is_empty() {
            crate::rag_ui::index_now(&ui, std::path::PathBuf::from(docs));
        }
    });

    ui.on_global_memory(|| {
        let path = crate::projects::ensure_global();
        let _ = std::process::Command::new("xdg-open")
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    });

    let weak = ui.as_weak();
    ui.on_group_dialog_toggle_extra(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        flip(&ui.get_group_dialog_extra_args(), index);
    });

    let weak = ui.as_weak();
    ui.on_group_dialog_toggle_removed(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        flip(&ui.get_group_dialog_removed_args(), index);
    });

    let weak = ui.as_weak();
    ui.on_group_dialog_pick_folder(move || {
        let weak = weak.clone();
        thread::spawn(move || {
            let Some(path) = rfd::FileDialog::new()
                .set_title(tr("Standard folder of the group"))
                .pick_folder()
            else {
                return;
            };
            let _ = weak.upgrade_in_event_loop(move |ui| match sandbox::check_root(&path) {
                Ok(root) => {
                    ui.set_group_dialog_folder(root.to_string_lossy().to_string().into());
                    ui.set_group_dialog_error("".into());
                }
                Err(e) => ui.set_group_dialog_error(trf("Can't use this folder: {}", &[&e]).into()),
            });
        });
    });

    let weak = ui.as_weak();
    ui.on_group_dialog_save(move || {
        let Some(ui) = weak.upgrade() else { return };
        match save_dialog(&ui) {
            Ok(group) => {
                ui.set_group_dialog_open(false);
                if ASSIGN_NEW.with(|assign| *assign.borrow()) {
                    choose(&ui, &group.id);
                }
                crate::chat::refresh_chat_list(&ui);
            }
            Err(e) => ui.set_group_dialog_error(e.into()),
        }
    });

    let weak = ui.as_weak();
    ui.on_group_dialog_delete(move || {
        let Some(ui) = weak.upgrade() else { return };
        let id = EDITING.with(|editing| editing.borrow().clone());
        match groups::delete(&id) {
            Ok(()) => {
                if ui.get_current_group() == id.as_str() {
                    set_current(&ui, "");
                }
                ui.set_group_dialog_open(false);
                ui.set_status(tr("Group deleted – its chats are now without group.").into());
                crate::chat::refresh_chat_list(&ui);
            }
            Err(e) => ui.set_group_dialog_error(e.into()),
        }
    });

    ui.on_group_open_memory(|id| {
        let id = if id.is_empty() {
            EDITING.with(|editing| editing.borrow().clone())
        } else {
            id.to_string()
        };
        if id.is_empty() {
            return;
        }
        let dir = groups::memory_dir(&id);
        let _ = fs::create_dir_all(&dir);
        let _ = Command::new("xdg-open")
            .arg(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    });

    let weak = ui.as_weak();
    ui.on_group_export(move |id, with_chats| {
        let Some(ui) = weak.upgrade() else { return };
        let name = groups::load(&id).map(|group| group.id).unwrap_or_default();
        let text = match groups::export(&id, with_chats) {
            Ok(text) => text,
            Err(e) => {
                ui.set_status(trf("Export failed: {}", &[&e]).into());
                return;
            }
        };
        let weak = weak.clone();
        thread::spawn(move || {
            let Some(path) = rfd::FileDialog::new()
                .set_title(tr("Export group"))
                .set_file_name(format!("{name}.dreamgroup"))
                .save_file()
            else {
                return;
            };
            let message = match fs::write(&path, text) {
                Ok(()) => trf("Group exported to {}", &[&path.display()]),
                Err(e) => trf("Could not write {}: {}", &[&path.display(), &e]),
            };
            let _ = weak.upgrade_in_event_loop(move |ui| ui.set_status(message.into()));
        });
    });

    let weak = ui.as_weak();
    ui.on_group_import(move || {
        let weak = weak.clone();
        thread::spawn(move || {
            let Some(path) = rfd::FileDialog::new()
                .set_title(tr("Import group"))
                .add_filter(tr("Dream-LLMChatBox group"), &["dreamgroup", "json"])
                .pick_file()
            else {
                return;
            };
            import_file(weak, path);
        });
    });
}

fn flip(list: &ModelRc<GroupArg>, index: i32) {
    if let Some(mut arg) = list.row_data(index as usize) {
        arg.on = !arg.on;
        list.set_row_data(index as usize, arg);
    }
}

fn chosen(list: &ModelRc<GroupArg>) -> Vec<String> {
    list.iter().filter(|arg| arg.on).map(|arg| arg.key.to_string()).collect()
}

fn group_args(enabled: bool, chosen: &[String]) -> ModelRc<GroupArg> {
    let list: Vec<GroupArg> = crate::start_args::load()
        .iter()
        .filter(|arg| arg.enabled == enabled && !arg.text.trim().is_empty())
        .map(|arg| {
            let key = crate::start_args::key(arg);
            GroupArg {
                on: chosen.contains(&key),
                title: crate::start_args::title(arg).into(),
                key: key.into(),
                text: arg.text.trim().into(),
            }
        })
        .collect();
    ModelRc::new(VecModel::from(list))
}

fn set_extra_args(ui: &AppWindow, extra: &[String], removed: &[String]) {
    ui.set_group_dialog_extra_args(group_args(false, extra));
    ui.set_group_dialog_removed_args(group_args(true, removed));
}

pub fn import_file(weak: slint::Weak<AppWindow>, path: PathBuf) {
    let text = fs::read_to_string(&path);
    let _ = weak.upgrade_in_event_loop(move |ui| {
        let message = match text {
            Err(e) => trf("Could not read {}: {}", &[&path.display(), &e]),
            Ok(text) => match groups::import(&text) {
                Ok(imported) => {
                    let rules = if imported.group.start_args.trim().is_empty() {
                        String::new()
                    } else {
                        tr(" Its rules are switched off – check them in the group settings.")
                    };
                    trf(
                        "Imported group \"{}\" with {} chat(s).{}",
                        &[&imported.group.name, &imported.chats, &rules],
                    )
                }
                Err(e) => trf("Import failed: {}", &[&e]),
            },
        };
        ui.set_status(message.into());
        crate::chat::refresh_chat_list(&ui);
    });
}

fn save_dialog(ui: &AppWindow) -> Result<Group, String> {
    let name = ui.get_group_dialog_name().trim().to_string();
    if name.is_empty() {
        return Err(tr("The group needs a name"));
    }
    let id = EDITING.with(|editing| editing.borrow().clone());
    let mut group = if id.is_empty() {
        groups::create(&name)?
    } else {
        groups::load(&id).ok_or(tr("Group not found"))?
    };
    group.name = name;
    let path = ui.get_group_dialog_folder().to_string();
    group.folder = (!path.is_empty()).then(|| Folder {
        path,
        writable: ui.get_group_dialog_writable(),
    });
    group.start_args = ui.get_group_dialog_rules().to_string();
    group.use_start_args = ui.get_group_dialog_use_rules();
    group.use_global_start_args = ui.get_group_dialog_use_global();
    group.extra_start_args = chosen(&ui.get_group_dialog_extra_args());
    group.removed_start_args = chosen(&ui.get_group_dialog_removed_args());
    group.test_env = ui.get_group_dialog_test_env();
    let docs = ui.get_group_dialog_docs().trim().to_string();
    group.docs_folder = (!docs.is_empty()).then_some(docs);
    group.memory_enabled = ui.get_group_dialog_memory();
    group.purpose = if ui.get_group_dialog_purpose() == 1 {
        crate::coding::CODING.to_string()
    } else {
        String::new()
    };
    groups::save(&group)?;
    Ok(group)
}

fn choose(ui: &AppWindow, id: &str) {
    set_current(ui, id);
    if groups::load(id).is_some_and(|group| group.purpose == crate::coding::CODING) {
        crate::coding_ui::set_coding(ui);
    }
    let chat = ui.get_current_chat_id().to_string();
    if !chat.is_empty() {
        let _ = storage::set_chat_group(&chat, id);
    }
    if let Some(folder) = groups::load(id).and_then(|group| group.folder)
        && folders::current(ui).is_empty()
        && sandbox::check_root(std::path::Path::new(&folder.path)).is_ok()
    {
        folders::set(ui, &[folder]);
    }
    crate::chat::refresh_chat_list(ui);
}

pub fn current_id() -> String {
    CURRENT.with(|current| current.borrow().clone())
}

pub fn set_current(ui: &AppWindow, id: &str) {
    let group = groups::load(id);
    ui.set_test_env_on(group.as_ref().is_some_and(|group| group.test_env));
    CURRENT.with(|current| *current.borrow_mut() = group.as_ref().map(|g| g.id.clone()).unwrap_or_default());
    ui.set_current_group(group.as_ref().map(|g| g.id.clone()).unwrap_or_default().into());
    ui.set_current_group_name(group.map(|g| g.name).unwrap_or_default().into());
    refresh(ui);
}

pub fn refresh(ui: &AppWindow) {
    let list = groups::list();
    let current = ui.get_current_group().to_string();
    let entries: Vec<GroupEntry> = list
        .iter()
        .map(|group| {
            let chats: Vec<ChatEntry> = storage::list_group_chats(&group.id)
                .into_iter()
                .map(|(id, title)| crate::chat_list::entry(id, title))
                .collect();
            GroupEntry {
                id: group.id.clone().into(),
                name: group.name.clone().into(),
                collapsed: group.collapsed,
                chats: ModelRc::new(VecModel::from(chats)),
                folder: group
                    .folder
                    .as_ref()
                    .map(|folder| sandbox::display_name(&folder.path))
                    .unwrap_or_default()
                    .into(),
                memory: group.memory_enabled,
                rules: group.use_start_args && !group.start_args.trim().is_empty(),
                global_args: group.use_global_start_args,
                selected: crate::chat_list::is_selected(&format!("{}{}", crate::chat_list::GROUP_PREFIX, group.id)),
            }
        })
        .collect();
    let mut options: Vec<slint::SharedString> = vec![tr("No group").into()];
    options.extend(list.iter().map(|group| group.name.clone().into()));
    let index = list
        .iter()
        .position(|group| group.id == current)
        .map_or(0, |position| position as i32 + 1);
    ui.set_groups(ModelRc::new(VecModel::from(entries)));
    ui.set_group_options(ModelRc::new(VecModel::from(options)));
    ui.set_group_index(index);
}
