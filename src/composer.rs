use crate::i18n::{tr, trf};
use crate::attachments::{self, Attachment, Kind};
use crate::audio::{self, Recorder};
use crate::settings;
use crate::speech;
use crate::AppWindow;
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

thread_local! {
    static PENDING: RefCell<Vec<Attachment>> = const { RefCell::new(Vec::new()) };
    static RECORDER: RefCell<Option<Recorder>> = const { RefCell::new(None) };
    static LEVEL_TIMER: Timer = Timer::default();
    static DEVICE_IDS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub fn setup(ui: &AppWindow) {
    let current = settings::load();
    ui.set_mic_languages(string_model(speech::LANGUAGES.iter().map(|(_, name)| *name)));
    ui.set_mic_language_index(
        speech::LANGUAGES
            .iter()
            .position(|(code, _)| *code == current.speech_language)
            .unwrap_or(0) as i32,
    );
    ui.set_speech_models(string_model(speech::MODELS.iter().map(|(_, _, label)| *label)));
    ui.set_speech_model_index(
        speech::MODELS
            .iter()
            .position(|(id, _, _)| *id == current.speech_model)
            .unwrap_or(0) as i32,
    );
    ui.set_mic_gain(current.mic_gain);
    ui.set_mic_live(current.live_speech);
    ui.set_mic_devices(string_model([tr("System default").as_str()].into_iter()));
    ui.set_mic_device_index(0);
    update_pending(ui);

    let weak = ui.as_weak();
    ui.on_attach_files(move || pick(weak.clone(), Pick::Files));
    let weak = ui.as_weak();
    ui.on_attach_folder(move || pick(weak.clone(), Pick::Folder));
    let weak = ui.as_weak();
    ui.on_attach_media(move || pick(weak.clone(), Pick::Media));

    let weak = ui.as_weak();
    ui.on_paste_clipboard(move || {
        weak.upgrade()
            .map(|ui| paste_clipboard(&ui))
            .unwrap_or(false)
    });

    let weak = ui.as_weak();
    ui.on_remove_attachment(move |index| {
        if let Some(ui) = weak.upgrade() {
            PENDING.with(|p| {
                let mut pending = p.borrow_mut();
                if (index as usize) < pending.len() {
                    pending.remove(index as usize);
                }
            });
            update_pending(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_mic_toggle(move || {
        if let Some(ui) = weak.upgrade() {
            toggle_recording(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_mic_panel(move |open| {
        if let Some(ui) = weak.upgrade() {
            if open {
                load_devices(&ui);
                if let Err(e) = ensure_stream(&ui) {
                    ui.set_mic_status(e.into());
                }
            } else if !ui.get_mic_recording() {
                stop_stream(&ui);
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_mic_device_selected(move |index| {
        if let Some(ui) = weak.upgrade() {
            let id = DEVICE_IDS.with(|ids| ids.borrow().get(index as usize).cloned().unwrap_or_default());
            settings::update(|s| s.mic_device = id);
            if !ui.get_mic_recording() {
                stop_stream(&ui);
                if ui.get_mic_panel_open()
                    && let Err(e) = ensure_stream(&ui)
                {
                    ui.set_mic_status(e.into());
                }
            }
        }
    });

    ui.on_mic_language_selected(|index| {
        if let Some((code, _)) = speech::LANGUAGES.get(index as usize) {
            settings::update(|s| s.speech_language = code.to_string());
        }
    });

    ui.on_speech_model_selected(|index| {
        if let Some((id, _, _)) = speech::MODELS.get(index as usize) {
            settings::update(|s| s.speech_model = id.to_string());
        }
    });

    ui.on_mic_live_changed(|on| settings::update(|s| s.live_speech = on));

    ui.on_mic_gain_changed(|gain| {
        RECORDER.with(|r| {
            if let Some(recorder) = r.borrow().as_ref() {
                recorder.set_gain(gain);
            }
        });
        settings::update(|s| s.mic_gain = gain);
    });
}

fn string_model<'a>(items: impl Iterator<Item = &'a str>) -> ModelRc<SharedString> {
    let list: Vec<SharedString> = items.map(SharedString::from).collect();
    ModelRc::new(Rc::new(VecModel::from(list)))
}

pub fn has_pending() -> bool {
    PENDING.with(|p| !p.borrow().is_empty())
}

pub fn take_pending(ui: &AppWindow) -> Vec<Attachment> {
    let taken = PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
    update_pending(ui);
    taken
}

pub fn restore_pending(ui: &AppWindow, list: Vec<Attachment>) {
    PENDING.with(|p| *p.borrow_mut() = list);
    update_pending(ui);
}

fn update_pending(ui: &AppWindow) {
    let names: Vec<SharedString> = PENDING.with(|p| {
        p.borrow()
            .iter()
            .map(|a| {
                let prefix = if a.kind == "image" { tr("Image: ") } else { String::new() };
                SharedString::from(format!("{prefix}{}", a.name))
            })
            .collect()
    });
    ui.set_pending_attachments(ModelRc::new(Rc::new(VecModel::from(names))));
}

fn add_pending(ui: &AppWindow, attachment: Attachment) {
    PENDING.with(|p| p.borrow_mut().push(attachment));
    update_pending(ui);
}

fn paste_clipboard(ui: &AppWindow) -> bool {
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return false;
    };
    if let Ok(files) = clipboard.get().file_list()
        && !files.is_empty()
    {
        add_paths(ui, files, false);
        return true;
    }
    let Ok(picture) = clipboard.get_image() else {
        return false;
    };
    let count = PENDING.with(|p| p.borrow().iter().filter(|a| a.kind == "image").count()) + 1;
    let name = format!("pasted-image-{count}.png");
    match attachments::image_from_rgba(
        name,
        picture.width as u32,
        picture.height as u32,
        picture.bytes.into_owned(),
    ) {
        Some(attachment) => {
            add_pending(ui, attachment);
            true
        }
        None => {
            ui.set_status(tr("Could not read the image from the clipboard.").into());
            true
        }
    }
}

enum Pick {
    Files,
    Folder,
    Media,
}

fn pick(weak: slint::Weak<AppWindow>, kind: Pick) {
    thread::spawn(move || {
        let paths: Vec<PathBuf> = match kind {
            Pick::Files => rfd::FileDialog::new()
                .set_title(tr("Add files"))
                .pick_files()
                .unwrap_or_default(),
            Pick::Folder => rfd::FileDialog::new()
                .set_title(tr("Add folder"))
                .pick_folders()
                .unwrap_or_default(),
            Pick::Media => rfd::FileDialog::new()
                .set_title(tr("Add images, video or audio"))
                .add_filter(tr("Images, video, audio"), &attachments::media_extensions())
                .pick_files()
                .unwrap_or_default(),
        };
        if paths.is_empty() {
            return;
        }
        let folder = matches!(kind, Pick::Folder);
        let _ = weak.upgrade_in_event_loop(move |ui| add_paths(&ui, paths, folder));
    });
}

fn add_paths(ui: &AppWindow, paths: Vec<PathBuf>, folder: bool) {
    let (folders, paths): (Vec<PathBuf>, Vec<PathBuf>) = if folder {
        (Vec::new(), paths)
    } else {
        paths.into_iter().partition(|path| path.is_dir())
    };
    if !folders.is_empty() {
        add_paths(ui, folders, true);
    }
    let mut media: Vec<PathBuf> = Vec::new();
    let mut quick: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !folder && matches!(attachments::kind_of(&path), Kind::Media) {
            media.push(path);
        } else {
            quick.push(path);
        }
    }

    if !quick.is_empty() {
        let weak = ui.as_weak();
        thread::spawn(move || {
            let results: Vec<Result<Attachment, String>> = quick
                .iter()
                .map(|path| {
                    if folder {
                        attachments::folder(path)
                    } else if matches!(attachments::kind_of(path), Kind::Image) {
                        attachments::image(path)
                    } else {
                        attachments::text_file(path)
                    }
                })
                .collect();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let mut errors = Vec::new();
                for result in results {
                    match result {
                        Ok(attachment) => add_pending(&ui, attachment),
                        Err(e) => errors.push(e),
                    }
                }
                if !errors.is_empty() {
                    ui.set_status(errors.join("  ·  ").into());
                }
            });
        });
    }

    if !media.is_empty() {
        transcribe_media(ui, media);
    }
}

fn speech_choice() -> (String, String) {
    let current = settings::load();
    (current.speech_model, current.speech_language)
}

fn transcribe_media(ui: &AppWindow, files: Vec<PathBuf>) {
    if ui.get_mic_busy() {
        ui.set_status(tr("Please wait until the current transcription is done.").into());
        return;
    }
    ui.set_mic_busy(true);
    let (model, language) = speech_choice();
    let weak = ui.as_weak();
    thread::spawn(move || {
        let model_path = download_model(&weak, &model);
        for path in files {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            set_mic_status(&weak, trf("Transcribing {}…", &[&name]));
            let result = model_path.clone().and_then(|model_path| {
                let samples = attachments::decode_audio(&path)?;
                speech::transcribe(&model_path, &samples, &language)
            });
            let _ = weak.upgrade_in_event_loop(move |ui| match result {
                Ok(text) => add_pending(&ui, attachments::transcript(&path, text)),
                Err(e) => ui.set_status(format!("{name}: {e}").into()),
            });
        }
        let _ = weak.upgrade_in_event_loop(|ui| {
            ui.set_mic_busy(false);
            ui.set_mic_status("".into());
        });
    });
}

fn download_model(weak: &slint::Weak<AppWindow>, model: &str) -> Result<PathBuf, String> {
    if !speech::model_path(model).exists() {
        set_mic_status(weak, tr("Downloading speech model…"));
    }
    speech::ensure_model(model, |percent| {
        set_mic_status(weak, trf("Downloading speech model… {}%", &[&percent]));
    })
}

fn set_mic_status(weak: &slint::Weak<AppWindow>, text: String) {
    let _ = weak.upgrade_in_event_loop(move |ui| ui.set_mic_status(text.into()));
}

fn load_devices(ui: &AppWindow) {
    let weak = ui.as_weak();
    thread::spawn(move || {
        let devices = audio::list_devices();
        let _ = weak.upgrade_in_event_loop(move |ui| {
            let saved = settings::load().mic_device;
            let mut ids = vec![String::new()];
            let mut names = vec![SharedString::from(tr("System default"))];
            for device in devices {
                ids.push(device.id);
                names.push(device.name.into());
            }
            let index = ids.iter().position(|id| *id == saved).unwrap_or(0);
            DEVICE_IDS.with(|d| *d.borrow_mut() = ids);
            ui.set_mic_devices(ModelRc::new(Rc::new(VecModel::from(names))));
            ui.set_mic_device_index(index as i32);
        });
    });
}

fn ensure_stream(ui: &AppWindow) -> Result<(), String> {
    let exists = RECORDER.with(|r| r.borrow().is_some());
    if exists {
        return Ok(());
    }
    let current = settings::load();
    let recorder = Recorder::start(&current.mic_device, ui.get_mic_gain())?;
    RECORDER.with(|r| *r.borrow_mut() = Some(recorder));
    ui.set_mic_status("".into());

    let weak = ui.as_weak();
    LEVEL_TIMER.with(|timer| {
        timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
            if let Some(ui) = weak.upgrade() {
                let level = RECORDER.with(|r| r.borrow().as_ref().map(|rec| rec.level()));
                ui.set_mic_level(level.unwrap_or(0.0));
            }
        });
    });
    Ok(())
}

fn stop_stream(ui: &AppWindow) {
    RECORDER.with(|r| *r.borrow_mut() = None);
    LEVEL_TIMER.with(|timer| timer.stop());
    ui.set_mic_level(0.0);
}

fn toggle_recording(ui: &AppWindow) {
    if ui.get_mic_busy() {
        return;
    }
    let recording = RECORDER.with(|r| r.borrow().as_ref().is_some_and(|rec| rec.is_recording()));

    if recording {
        let samples = RECORDER.with(|r| {
            r.borrow()
                .as_ref()
                .map(|rec| rec.finish_recording())
                .unwrap_or_default()
        });
        ui.set_mic_recording(false);
        crate::live_speech::stop(ui);
        if !ui.get_mic_panel_open() {
            stop_stream(ui);
        }
        transcribe_into_draft(ui, samples);
        return;
    }

    if let Err(e) = ensure_stream(ui) {
        ui.set_status(trf("Microphone: {}", &[&e]).into());
        return;
    }
    RECORDER.with(|r| {
        if let Some(rec) = r.borrow().as_ref() {
            rec.begin_recording();
        }
    });
    ui.set_mic_recording(true);
    start_live(ui);
}

fn start_live(ui: &AppWindow) {
    if !settings::load().live_speech {
        return;
    }
    let (model, language) = speech_choice();
    let path = speech::model_path(&model);
    if !path.exists() {
        return;
    }
    if let Some(live) = RECORDER.with(|r| r.borrow().as_ref().map(Recorder::live)) {
        crate::live_speech::start(ui, live, path, language);
    }
}

fn transcribe_into_draft(ui: &AppWindow, samples: Vec<f32>) {
    ui.set_mic_busy(true);
    let (model, language) = speech_choice();
    let weak = ui.as_weak();
    thread::spawn(move || {
        let result = download_model(&weak, &model).and_then(|path| {
            set_mic_status(&weak, tr("Transcribing…"));
            speech::transcribe(&path, &samples, &language)
        });
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.set_mic_busy(false);
            ui.set_mic_status("".into());
            match result {
                Ok(text) if !text.is_empty() => {
                    let draft = ui.get_draft().to_string();
                    let joined = if draft.trim().is_empty() {
                        text
                    } else {
                        format!("{} {text}", draft.trim_end())
                    };
                    ui.set_draft(joined.into());
                }
                Ok(_) => ui.set_status(tr("No speech recognized.").into()),
                Err(e) => ui.set_status(trf("Speech recognition: {}", &[&e]).into()),
            }
        });
    });
}
