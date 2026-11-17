use crate::i18n::{tr, trf};
use crate::AppWindow;
use crate::coding::{self, CODING, HAS_ARCHITECTURE, Mode, NEW_PROJECT, NO_ARCHITECTURE};
use crate::folders;
use crate::sandbox::{self, Folder};
use slint::{ComponentHandle, Model};
use std::cell::RefCell;
use std::path::Path;

thread_local! {
    static MODE: RefCell<Mode> = RefCell::new(Mode::default());
}

pub fn setup(ui: &AppWindow) {
    let weak = ui.as_weak();
    ui.on_purpose_selected(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let mut mode = current();
        mode.purpose = if index == 1 { CODING.to_string() } else { String::new() };
        set(&ui, mode);
    });

    let weak = ui.as_weak();
    ui.on_coding_choice_selected(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        let mut mode = current();
        mode.coding = (index + 1).clamp(1, 3) as u8;
        set(&ui, mode);
    });
}

pub fn current() -> Mode {
    MODE.with(|mode| mode.borrow().clone())
}

pub fn set(ui: &AppWindow, mode: Mode) {
    MODE.with(|current| *current.borrow_mut() = mode);
    refresh(ui);
}

pub fn reset(ui: &AppWindow) {
    set(ui, Mode::default());
}

pub fn set_coding(ui: &AppWindow) {
    let mut mode = current();
    if !mode.is_coding() {
        mode.purpose = CODING.to_string();
        set(ui, mode);
    }
}

pub fn project_folder(list: &[Folder]) -> Option<Folder> {
    list.first().cloned()
}

pub fn before_send(ui: &AppWindow) {
    let mut mode = current();
    let has_messages = ui.get_messages().row_count() > 0;
    if !mode.is_coding() || !has_messages || mode.coding == HAS_ARCHITECTURE {
        return;
    }
    let found = project_folder(&folders::current(ui))
        .is_some_and(|folder| coding::find_architecture(Path::new(&folder.path)).is_some());
    if found {
        mode.coding = HAS_ARCHITECTURE;
        set(ui, mode);
    }
}

pub fn refresh(ui: &AppWindow) {
    let mode = current();
    ui.set_purpose_index(i32::from(mode.is_coding()));
    let folder = project_folder(&folders::current(ui));
    let architecture = folder
        .as_ref()
        .and_then(|folder| coding::find_architecture(Path::new(&folder.path)));
    let choice = mode.choice(architecture.is_some());
    ui.set_coding_choice(choice as i32 - 1);
    ui.set_coding_folder(
        folder
            .as_ref()
            .map(|folder| sandbox::display_name(&folder.path))
            .unwrap_or_default()
            .into(),
    );
    let hint = match (&folder, &architecture, choice) {
        (None, _, _) => tr("No project folder shared yet – the first shared folder is the project."),
        (Some(folder), Some(path), _) => {
            let lines = coding::line_count(path);
            let unit = if lines == 1 { tr("line") } else { tr("lines") };
            trf(
                "Found {} ({} {}) – the model gets it first.",
                &[&coding::relative(Path::new(&folder.path), path), &lines, &unit],
            )
        }
        (Some(_), None, HAS_ARCHITECTURE) => tr("No ARCHITEKTUR.md found in the folder or in docs/."),
        (Some(_), None, NEW_PROJECT) => {
            tr("The model asks a few questions, then writes ARCHITEKTUR.md.")
        }
        (Some(_), None, _) => tr("The model looks at the folder and writes ARCHITEKTUR.md first."),
    };
    ui.set_coding_hint(hint.into());
    let read_only = folder.as_ref().is_some_and(|folder| !folder.writable);
    let warning = if read_only && architecture.is_none() && (choice == NO_ARCHITECTURE || choice == NEW_PROJECT) {
        tr("Allow writing (click 'read only' on the folder chip) so the model can create ARCHITEKTUR.md")
    } else {
        String::new()
    };
    ui.set_coding_warning(warning.into());
}
