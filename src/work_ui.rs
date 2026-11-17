use crate::i18n::{tr, trf};
use crate::folders;
use crate::storage;
use crate::workspace;
use crate::AppWindow;
use slint::ComponentHandle;
use std::path::PathBuf;

fn paths(ui: &AppWindow) -> Option<(PathBuf, PathBuf)> {
    let chat = ui.get_current_chat_id().to_string();
    if chat.is_empty() {
        return None;
    }
    let group = storage::load_chat(&chat).map(|file| file.group).unwrap_or_default();
    let work = workspace::chat_dir(&group, &chat);
    let project = folders::current(ui)
        .into_iter()
        .find(|folder| !workspace::is_inside(&folder.path))
        .map(|folder| PathBuf::from(folder.path))?;
    work.exists().then_some((project, work))
}

pub fn refresh(ui: &AppWindow) {
    let count = paths(ui).map_or(0, |(project, work)| workspace::changes(&project, &work).len());
    ui.set_work_changes(count as i32);
}

fn show(ui: &AppWindow) {
    match paths(ui) {
        Some((project, work)) => {
            let changes = workspace::changes(&project, &work);
            ui.set_work_changes(changes.len() as i32);
            ui.set_work_diff(workspace::report(&changes).into());
            ui.set_work_project(project.display().to_string().into());
        }
        None => {
            ui.set_work_changes(0);
            ui.set_work_diff("".into());
        }
    }
}

fn test_env_target(ui: &AppWindow) -> (PathBuf, Option<PathBuf>) {
    if ui.get_current_chat_id().is_empty() {
        ui.set_current_chat_id(storage::new_id().into());
    }
    let chat = ui.get_current_chat_id().to_string();
    let group = ui.get_current_group().to_string();
    let project = folders::current(ui)
        .into_iter()
        .find(|folder| !workspace::is_inside(&folder.path))
        .map(|folder| PathBuf::from(folder.path));
    (workspace::chat_dir(&group, &chat), project)
}

fn open_test_env(ui: &AppWindow) {
    if ui.get_busy() {
        ui.set_status(tr("Wait until the answer is finished.").into());
        return;
    }
    let (dir, project) = test_env_target(ui);
    let mut text = trf("Folder: {}\n\n", &[&dir.display()]);
    if dir.exists() {
        text.push_str(&tr("This chat already has a test environment. Accept to give the model access to it again.\n\n"));
    } else if let Some(project) = &project {
        text.push_str(&trf(
            "A copy of {} is made there (without .git, target, node_modules).\n\n",
            &[&project.display()],
        ));
    } else {
        text.push_str(&tr("No project folder is shared yet, so the folder starts empty. Share your project first if the model should get a copy.\n\n"));
    }
    text.push_str(&tr("The model may create, change, build and run everything in this folder. Your project stays untouched – you take changes over with “Apply to project”. The folder is deleted together with the chat or its group. Change the base folder in Settings."));
    ui.set_test_env_text(text.into());
    ui.set_test_env_open(true);
}

fn accept_test_env(ui: &AppWindow) {
    let (dir, project) = test_env_target(ui);
    if !dir.exists() {
        let result = match &project {
            Some(project) => workspace::copy_project(project, &dir).map(|count| trf("Test environment ready: {} files copied to {}", &[&count, &dir.display()])),
            None => std::fs::create_dir_all(&dir).map(|_| trf("Test environment ready: {}", &[&dir.display()])).map_err(|e| e.to_string()),
        };
        match result {
            Ok(note) => ui.set_status(note.into()),
            Err(e) => {
                ui.set_status(trf("Could not create the test environment: {}", &[&e]).into());
                ui.set_test_env_open(false);
                return;
            }
        }
    }
    let path = dir.to_string_lossy().to_string();
    let mut list = folders::current(ui);
    list.retain(|folder| folder.path != path);
    list.insert(0, crate::sandbox::Folder { path, writable: true });
    folders::changed(ui, &list);
    ui.set_test_env_open(false);
    refresh(ui);
}

pub fn ensure_group_test_env(ui: &AppWindow) {
    let group = ui.get_current_group().to_string();
    let wanted = ui.get_test_env_on() || crate::groups::load(&group).is_some_and(|group| group.test_env);
    if !wanted {
        return;
    }
    if folders::current(ui).iter().any(|folder| workspace::is_inside(&folder.path)) {
        return;
    }
    accept_test_env(ui);
}

pub fn setup(ui: &AppWindow) {
    let weak = ui.as_weak();
    ui.on_test_env(move || {
        if let Some(ui) = weak.upgrade() {
            open_test_env(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_test_env_accept(move || {
        if let Some(ui) = weak.upgrade() {
            accept_test_env(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_open_work_changes(move || {
        if let Some(ui) = weak.upgrade() {
            show(&ui);
            ui.set_work_open(true);
        }
    });

    let weak = ui.as_weak();
    ui.on_apply_work_changes(move || {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_busy() {
            ui.set_status(tr("Wait until the answer is finished.").into());
            return;
        }
        let Some((project, work)) = paths(&ui) else { return };
        let changes = workspace::changes(&project, &work);
        match workspace::apply(&project, &work, &changes) {
            Ok(count) => ui.set_status(trf("{} files copied into {}", &[&count, &project.display()]).into()),
            Err(e) => ui.set_status(trf("Could not apply the changes: {}", &[&e]).into()),
        }
        show(&ui);
        ui.set_work_open(false);
    });
}
