use crate::i18n::{tr, trf};
use crate::markdown;
use crate::settings;
use crate::tts::{self, Speaker};
use crate::{AppWindow, VoiceEntry};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::thread;

thread_local! {
    static SPEAKER: RefCell<Option<Speaker>> = const { RefCell::new(None) };
    static GENERATION: Cell<u64> = const { Cell::new(0) };
}

pub fn setup(ui: &AppWindow) {
    ui.set_speaking_index(-1);
    refresh_voices(ui);

    let weak = ui.as_weak();
    ui.on_speak(move |index| {
        if let Some(ui) = weak.upgrade() {
            speak(&ui, index);
        }
    });

    let weak = ui.as_weak();
    ui.on_voice_selected(move |id| {
        if let Some(ui) = weak.upgrade() {
            let id = id.to_string();
            let installed = tts::voice_path(&id).exists();
            settings::update(|s| s.tts_voice = id.clone());
            if installed {
                refresh_voices(&ui);
            } else {
                download(&ui, id, None);
            }
        }
    });

    ui.on_install_piper(|command| {
        let script = format!("{command}; echo; echo 'Done. Press Enter to close.'; read _");
        let _ = crate::terminal::run(&script);
    });

    let weak = ui.as_weak();
    ui.on_voice_add_file(move || {
        let weak = weak.clone();
        thread::spawn(move || {
            let Some(file) = rfd::FileDialog::new()
                .set_title(tr("Add Piper voice (.onnx)"))
                .add_filter(tr("Piper voice"), &["onnx"])
                .pick_file()
            else {
                return;
            };
            let result = tts::add_voice_file(&file);
            let _ = weak.upgrade_in_event_loop(move |ui| match result {
                Ok(id) => {
                    settings::update(|s| s.tts_voice = id);
                    refresh_voices(&ui);
                }
                Err(e) => ui.set_status(trf("Voice: {}", &[&e]).into()),
            });
        });
    });
}

fn current_voice() -> String {
    let chosen = settings::load().tts_voice;
    if chosen.is_empty() {
        tts::default_voice()
    } else {
        chosen
    }
}

pub fn refresh_voices(ui: &AppWindow) {
    let active = current_voice();
    let list: Vec<VoiceEntry> = tts::voices()
        .into_iter()
        .map(|voice| VoiceEntry {
            active: voice.id == active,
            id: voice.id.into(),
            label: voice.label.into(),
            installed: voice.installed,
        })
        .collect();
    ui.set_voices(ModelRc::new(Rc::new(VecModel::from(list))));
}

fn stop(ui: &AppWindow) {
    SPEAKER.with(|s| {
        if let Some(speaker) = s.borrow_mut().take() {
            speaker.stop();
        }
    });
    ui.set_speaking_index(-1);
}

fn speak(ui: &AppWindow, index: i32) {
    let was_speaking = ui.get_speaking_index();
    stop(ui);
    if was_speaking == index {
        return;
    }
    let Some(message) = ui.get_messages().row_data(index as usize) else {
        return;
    };
    let text = markdown::without_code(&message.text);
    if text.trim().is_empty() {
        return;
    }
    if !tts::piper_installed() {
        ui.set_piper_command(tts::install_command().into());
        ui.set_piper_dialog_open(true);
        return;
    }
    let voice = current_voice();
    if !tts::voice_path(&voice).exists() {
        download(ui, voice, Some((index, text)));
        return;
    }
    start(ui, index, text, voice);
}

fn start(ui: &AppWindow, index: i32, text: String, voice: String) {
    let generation = GENERATION.with(|g| {
        g.set(g.get() + 1);
        g.get()
    });
    let weak = ui.as_weak();
    let result = tts::speak(&text, &voice, move |result| {
        let _ = weak.upgrade_in_event_loop(move |ui| {
            if GENERATION.with(|g| g.get()) != generation {
                return;
            }
            SPEAKER.with(|s| s.borrow_mut().take());
            ui.set_speaking_index(-1);
            if let Err(e) = result {
                ui.set_status(trf("Read aloud: {}", &[&e]).into());
            }
        });
    });
    match result {
        Ok(speaker) => {
            SPEAKER.with(|s| *s.borrow_mut() = Some(speaker));
            ui.set_speaking_index(index);
        }
        Err(e) if e == tts::PIPER_MISSING => {
            ui.set_piper_command(tts::install_command().into());
            ui.set_piper_dialog_open(true);
        }
        Err(e) => ui.set_status(e.into()),
    }
}

fn download(ui: &AppWindow, id: String, then_speak: Option<(i32, String)>) {
    if ui.get_voice_downloading() {
        return;
    }
    ui.set_voice_downloading(true);
    ui.set_mic_status(trf("Downloading voice {}…", &[&id]).into());
    let weak = ui.as_weak();
    thread::spawn(move || {
        let progress_weak = weak.clone();
        let progress_id = id.clone();
        let result = tts::download(&id, move |percent| {
            let text = trf("Downloading voice {}… {}%", &[&progress_id, &percent]);
            let _ = progress_weak.upgrade_in_event_loop(move |ui| ui.set_mic_status(text.into()));
        });
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.set_voice_downloading(false);
            ui.set_mic_status("".into());
            refresh_voices(&ui);
            match result {
                Ok(()) => {
                    if let Some((index, text)) = then_speak {
                        start(&ui, index, text, id);
                    }
                }
                Err(e) => ui.set_status(trf("Voice download: {}", &[&e]).into()),
            }
        });
    });
}
