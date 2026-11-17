use crate::chat;
use crate::sandbox::{self, Folder};
use crate::{AppWindow, SharedFolder};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::path::PathBuf;
use std::thread;

pub fn setup(ui: &AppWindow) {
    let weak = ui.as_weak();
    ui.on_share_folder(move || {
        let weak = weak.clone();
        thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title(crate::i18n::tr("Give the model access to a folder"))
                .pick_folder();
            if let Some(path) = picked {
                let _ = weak.upgrade_in_event_loop(move |ui| add(&ui, path));
            }
        });
    });

    let weak = ui.as_weak();
    ui.on_remove_shared_folder(move |index| {
        if let Some(ui) = weak.upgrade() {
            let mut list = current(&ui);
            if (index as usize) < list.len() {
                list.remove(index as usize);
            }
            changed(&ui, &list);
        }
    });

    let weak = ui.as_weak();
    ui.on_toggle_folder_write(move |index| {
        if let Some(ui) = weak.upgrade() {
            let mut list = current(&ui);
            if let Some(folder) = list.get_mut(index as usize) {
                folder.writable = !folder.writable;
            }
            changed(&ui, &list);
        }
    });
}

fn add(ui: &AppWindow, path: PathBuf) {
    match sandbox::check_root(&path) {
        Ok(root) => {
            let mut list = current(ui);
            let path = root.to_string_lossy().to_string();
            if !list.iter().any(|folder| folder.path == path) {
                list.push(Folder {
                    path,
                    writable: false,
                });
            }
            changed(ui, &list);
            ui.set_status("".into());
        }
        Err(e) => ui.set_status(crate::i18n::trf("Can't share this folder: {}", &[&e]).into()),
    }
}

pub fn changed(ui: &AppWindow, list: &[Folder]) {
    set(ui, list);
    if !ui.get_current_chat_id().is_empty() && !ui.get_busy() && ui.get_messages().row_count() > 0 {
        chat::save_current_chat(ui);
    }
}

pub fn current(ui: &AppWindow) -> Vec<Folder> {
    ui.get_shared_folders()
        .iter()
        .map(|folder| Folder {
            path: folder.path.to_string(),
            writable: folder.writable,
        })
        .collect()
}

pub fn set(ui: &AppWindow, list: &[Folder]) {
    let entries: Vec<SharedFolder> = list
        .iter()
        .map(|folder| SharedFolder {
            name: if crate::workspace::is_inside(&folder.path) {
                crate::i18n::tr("Test environment").into()
            } else {
                sandbox::display_name(&folder.path).into()
            },
            path: folder.path.clone().into(),
            writable: folder.writable,
        })
        .collect();
    ui.set_shared_folders(ModelRc::new(VecModel::from(entries)));
    crate::coding_ui::refresh(ui);
    crate::auto_sync::refresh(ui);
}

pub fn label(list: &[Folder]) -> String {
    list.iter()
        .map(|folder| {
            format!(
                "📁 {} ({})",
                if crate::workspace::is_inside(&folder.path) {
                    "🧪 test environment".to_string()
                } else {
                    sandbox::display_name(&folder.path)
                },
                if folder.writable { "read + write" } else { "read only" }
            )
        })
        .collect::<Vec<_>>()
        .join("  ")
}
