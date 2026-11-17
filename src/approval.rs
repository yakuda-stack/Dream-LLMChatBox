use crate::AppWindow;
use crate::i18n::{tr, trf};
use crate::commands::Approval;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static PENDING: Mutex<Option<Sender<Approval>>> = Mutex::new(None);

pub fn setup(ui: &AppWindow) {
    ui.on_approval_answer(|choice| {
        let answer = match choice {
            0 => Approval::Once,
            1 => Approval::Chat,
            _ => Approval::No,
        };
        if let Some(sender) = PENDING.lock().ok().and_then(|mut pending| pending.take()) {
            let _ = sender.send(answer);
        }
    });
}

pub fn ask(
    weak: &slint::Weak<AppWindow>,
    cancel: &Arc<AtomicBool>,
    command: &str,
    directory: &Path,
    read_only: bool,
) -> Approval {
    let warning = if read_only {
        tr("The folder is shared read only, but a command can still change files there.")
    } else {
        String::new()
    };
    show(
        weak,
        cancel,
        &tr("The model wants to run a command:"),
        &format!("$ {command}"),
        &trf("in {}", &[&directory.display()]),
        &warning,
    )
}

pub fn show(
    weak: &slint::Weak<AppWindow>,
    cancel: &Arc<AtomicBool>,
    title: &str,
    text: &str,
    note: &str,
    warning: &str,
) -> Approval {
    let (sender, receiver) = mpsc::channel();
    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some(sender);
    }
    let (title, text, note, warning) =
        (title.to_string(), text.to_string(), note.to_string(), warning.to_string());
    let _ = weak.upgrade_in_event_loop(move |ui| {
        ui.set_approval_title(title.into());
        ui.set_approval_command(text.into());
        ui.set_approval_folder(note.into());
        ui.set_approval_warning(warning.into());
        ui.set_approval_open(true);
    });
    let answer = loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(answer) => break answer,
            Err(RecvTimeoutError::Timeout) if cancel.load(Ordering::SeqCst) => break Approval::No,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break Approval::No,
        }
    };
    if let Ok(mut pending) = PENDING.lock() {
        *pending = None;
    }
    let _ = weak.upgrade_in_event_loop(|ui| ui.set_approval_open(false));
    answer
}
