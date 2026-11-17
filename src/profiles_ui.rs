use crate::i18n::{tr, trf};
use crate::profiles::{self, Key, Profile};
use crate::{AppWindow, ProfileEntry};
use crate::{activity, chat, settings, start_args_page, storage};
use slint::{ComponentHandle, Image, ModelRc, Rgba8Pixel, SharedPixelBuffer, Timer, TimerMode, VecModel};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};
use std::time::Duration;

const AUTOSAVE_SECS: u64 = 10;

struct Session {
    id: String,
    dir: PathBuf,
    key: Option<Key>,
    fingerprint: u64,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static AUTOSAVE: Timer = Timer::default();
}

static ACTIVE: RwLock<String> = RwLock::new(String::new());
static GENERATION: AtomicU64 = AtomicU64::new(0);
static WRITTEN: Mutex<u64> = Mutex::new(0);

fn base() -> PathBuf {
    profiles::profiles_dir()
}

fn active_id() -> String {
    ACTIVE.read().map(|id| id.clone()).unwrap_or_default()
}

fn set_active_id(id: &str) {
    if let Ok(mut active) = ACTIVE.write() {
        *active = id.to_string();
    }
}

pub fn active() -> Option<Profile> {
    profiles::load_in(&base(), &active_id())
}

fn next_generation() -> u64 {
    GENERATION.fetch_add(1, Ordering::SeqCst) + 1
}

fn store_generation(id: &str, key: &Key, archive: &[u8], generation: u64) -> Result<(), String> {
    let mut written = WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
    if generation <= *written {
        return Ok(());
    }
    profiles::write_data_in(&base(), id, key, archive)?;
    *written = generation;
    Ok(())
}

fn store(id: &str, key: &Key, archive: &[u8]) -> Result<(), String> {
    store_generation(id, key, archive, next_generation())
}

fn with_writer<T>(change: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let mut written = WRITTEN.lock().unwrap_or_else(|e| e.into_inner());
    let result = change();
    *written = next_generation();
    result
}

pub fn setup(ui: &AppWindow) {
    let current = settings::load();
    if let Err(e) = profiles::migrate_in(&storage::base_dir(), &config_dir(), &current.user_name, &current.user_about) {
        ui.set_status(trf("Could not move your data into the profile folder: {}", &[&e]).into());
    }
    if !current.user_name.is_empty() || !current.user_about.is_empty() {
        settings::update(|s| {
            s.user_name.clear();
            s.user_about.clear();
        });
    }
    let list = profiles::list_in(&base());
    let start = list
        .iter()
        .find(|p| p.id == current.active_profile)
        .or_else(|| list.first())
        .cloned();
    match start {
        Some(profile) if profile.locked => {
            storage::set_profile_dir(None, false);
            show_unlock(ui, &profile.id, "");
        }
        Some(profile) => {
            if let Err(e) = open(ui, &profile.id, None, false) {
                storage::set_profile_dir(None, false);
                ui.set_status(trf("Could not open the profile: {}", &[&e]).into());
            }
        }
        None => {
            storage::set_profile_dir(None, false);
            ui.set_status(tr("No profile found.").into());
        }
    }
    show_active(ui);
    wire(ui);

    let weak = ui.as_weak();
    AUTOSAVE.with(|timer| {
        timer.start(TimerMode::Repeated, Duration::from_secs(AUTOSAVE_SECS), move || autosave(&weak));
    });
}

fn wire(ui: &AppWindow) {
    let weak = ui.as_weak();
    ui.on_open_profiles(move || {
        if let Some(ui) = weak.upgrade() {
            refresh_list(&ui);
            ui.set_profiles_error("".into());
            ui.set_profiles_open(true);
        }
    });

    let weak = ui.as_weak();
    ui.on_switch_profile(move |id| {
        if let Some(ui) = weak.upgrade() {
            switch_profile(&ui, &id);
        }
    });

    let weak = ui.as_weak();
    ui.on_create_profile(move |name| {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_busy() {
            ui.set_profiles_error(tr("Wait until the answer is finished.").into());
            return;
        }
        match profiles::create_in(&base(), &name) {
            Ok(profile) => switch_to(&ui, &profile.id, None),
            Err(e) => {
                ui.set_unlock_error(e.as_str().into());
                ui.set_profiles_error(e.into());
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_unlock_profile(move |password| {
        let Some(ui) = weak.upgrade() else { return };
        let id = ui.get_unlock_id().to_string();
        let Some(profile) = profiles::load_in(&base(), &id) else {
            ui.set_unlock_error(tr("Profile not found.").into());
            return;
        };
        match profiles::unlock_key(&profile, &password) {
            Ok(key) => {
                ui.set_unlock_password("".into());
                switch_to(&ui, &id, Some(key));
                if active_id() == id {
                    match remove_password() {
                        Ok(()) => ui.set_status(tr("Profile passwords are gone. This profile is open without a password now.").into()),
                        Err(e) => ui.set_status(trf("Could not remove the old password: {}", &[&e]).into()),
                    }
                    show_active(&ui);
                }
            }
            Err(e) => ui.set_unlock_error(e.into()),
        }
    });

    let weak = ui.as_weak();
    ui.on_profile_pick_picture(move || {
        if let Some(ui) = weak.upgrade() {
            pick_picture(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_profile_remove_picture(move || {
        if let Some(ui) = weak.upgrade() {
            let _ = fs::remove_file(profiles::avatar_path_in(&base(), &active_id()));
            show_active(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_profile_delete(move || {
        if let Some(ui) = weak.upgrade() {
            delete_active(&ui);
        }
    });
}

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dream-llmchatbox")
}

fn load_picture(path: &Path) -> Option<Image> {
    if !path.exists() {
        return None;
    }
    let rgba = image::open(path).ok()?.to_rgba8();
    let buffer = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height());
    Some(Image::from_rgba8(buffer))
}

fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn refresh_list(ui: &AppWindow) {
    let active = active_id();
    let entries: Vec<ProfileEntry> = profiles::list_in(&base())
        .into_iter()
        .map(|profile| {
            let picture = load_picture(&profiles::avatar_path_in(&base(), &profile.id));
            ProfileEntry {
                active: profile.id == active,
                initial: initial(&profile.name).into(),
                has_picture: picture.is_some(),
                picture: picture.unwrap_or_default(),
                locked: profile.locked,
                id: profile.id.into(),
                name: profile.name.into(),
            }
        })
        .collect();
    ui.set_profile_count(entries.len() as i32);
    ui.set_profiles(ModelRc::new(VecModel::from(entries)));
}

fn show_active(ui: &AppWindow) {
    let profile = active();
    let name = profile.as_ref().map(|p| p.name.clone()).unwrap_or_default();
    ui.set_user_name(crate::profile::display_name(&name).into());
    let picture = load_picture(&profiles::avatar_path_in(&base(), &active_id()));
    ui.set_has_profile_picture(picture.is_some());
    ui.set_profile_picture(picture.unwrap_or_default());
    ui.set_profile_initial(initial(&name).into());
    ui.set_profile_locked(profile.is_some_and(|p| p.locked));
    refresh_list(ui);
}

fn show_unlock(ui: &AppWindow, id: &str, error: &str) {
    let Some(profile) = profiles::load_in(&base(), id) else { return };
    let picture = load_picture(&profiles::avatar_path_in(&base(), id));
    ui.set_unlock_id(id.into());
    ui.set_unlock_name(profile.name.as_str().into());
    ui.set_unlock_initial(initial(&profile.name).into());
    ui.set_unlock_has_picture(picture.is_some());
    ui.set_unlock_picture(picture.unwrap_or_default());
    ui.set_unlock_error(error.into());
    ui.set_unlock_password("".into());
    ui.set_unlock_can_cancel(SESSION.with(|s| s.borrow().is_some()));
    refresh_list(ui);
    ui.set_profiles_open(false);
    ui.set_settings_page(0);
    ui.set_unlock_open(true);
}

fn switch_profile(ui: &AppWindow, id: &str) {
    if SESSION.with(|s| s.borrow().as_ref().is_some_and(|s| s.id == id)) {
        ui.set_profiles_open(false);
        ui.set_unlock_open(false);
        return;
    }
    if ui.get_busy() {
        ui.set_profiles_error(tr("Wait until the answer is finished.").into());
        ui.set_unlock_error(tr("Wait until the answer is finished.").into());
        return;
    }
    match profiles::load_in(&base(), id) {
        Some(profile) if profile.locked => show_unlock(ui, id, ""),
        Some(_) => switch_to(ui, id, None),
        None => ui.set_profiles_error(tr("Profile not found.").into()),
    }
}

fn switch_to(ui: &AppWindow, id: &str, key: Option<Key>) {
    if ui.get_busy() {
        ui.set_status(tr("Wait until the answer is finished.").into());
        return;
    }
    ui.set_status("".into());
    if let Err(e) = close_current() {
        ui.set_status(trf("Could not lock the current profile: {}", &[&e]).into());
        return;
    }
    match open(ui, id, key, true) {
        Ok(()) => {
            ui.set_profiles_open(false);
            ui.set_unlock_open(false);
        }
        Err(e) => {
            storage::set_profile_dir(None, false);
            reload_data(ui);
            show_active(ui);
            if profiles::load_in(&base(), id).is_some_and(|p| p.locked) {
                show_unlock(ui, id, &trf("Could not open the profile: {}", &[&e]));
            } else {
                ui.set_profiles_error(trf("Could not open the profile: {}", &[&e]).into());
                refresh_list(ui);
                ui.set_profiles_open(true);
            }
        }
    }
}

fn open(ui: &AppWindow, id: &str, key: Option<Key>, reload: bool) -> Result<(), String> {
    let profile = profiles::load_in(&base(), id).ok_or_else(|| tr("Profile not found"))?;
    let mut leftover = false;
    let dir = if profile.locked {
        let key = key.as_ref().ok_or_else(|| tr("The profile is locked"))?;
        let (work, in_ram) = profiles::work_dir(id);
        if !in_ram {
            eprintln!(
                "Warning: XDG_RUNTIME_DIR is not available, the unlocked profile is kept in {}",
                work.display()
            );
        }
        leftover = profiles::unlock_into(&base(), id, key, &work)?;
        work
    } else {
        let dir = profiles::profile_dir_in(&base(), id);
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        dir
    };
    storage::set_profile_dir(Some(dir.clone()), id == profiles::DEFAULT_ID);
    let key = if profile.locked { key } else { None };
    if leftover && let Some(key) = &key {
        store(id, key, &profiles::pack(&dir)?)?;
        ui.set_status(tr("The app was not closed properly last time. The unlocked data of that session was used and is encrypted again.").into());
    }
    SESSION.with(|s| {
        *s.borrow_mut() = Some(Session {
            id: id.to_string(),
            fingerprint: profiles::fingerprint(&dir),
            dir,
            key,
        })
    });
    set_active_id(id);
    settings::update(|s| s.active_profile = id.to_string());
    show_active(ui);
    if reload {
        reload_data(ui);
    }
    Ok(())
}

fn close_current() -> Result<(), String> {
    let Some(session) = SESSION.with(|s| s.borrow_mut().take()) else {
        return Ok(());
    };
    storage::close();
    if let Some(key) = &session.key {
        let result = profiles::pack(&session.dir).and_then(|archive| store(&session.id, key, &archive));
        if let Err(e) = result {
            SESSION.with(|s| *s.borrow_mut() = Some(session));
            return Err(e);
        }
        let _ = fs::remove_dir_all(&session.dir);
    }
    storage::set_profile_dir(None, false);
    set_active_id("");
    Ok(())
}

pub fn shutdown() {
    AUTOSAVE.with(|timer| timer.stop());
    if let Err(e) = close_current() {
        eprintln!("Could not encrypt the profile: {e}. The unlocked data stays in place and is used on the next unlock.");
    }
}

fn reload_data(ui: &AppWindow) {
    ui.set_settings_page(0);
    activity::clear();
    ui.set_chat_offset(0);
    ui.invoke_new_chat();
    chat::refresh_chat_list(ui);
    start_args_page::refresh(ui);
    activity::show(ui);
}

fn autosave(weak: &slint::Weak<AppWindow>) {
    let job = SESSION.with(|s| {
        let mut slot = s.borrow_mut();
        let session = slot.as_mut()?;
        let key = session.key?;
        let now = profiles::fingerprint(&session.dir);
        if now == session.fingerprint {
            return None;
        }
        let archive = profiles::pack(&session.dir).ok()?;
        session.fingerprint = now;
        Some((session.id.clone(), key, archive, next_generation()))
    });
    let Some((id, key, archive, generation)) = job else { return };
    let weak = weak.clone();
    std::thread::spawn(move || {
        if let Err(e) = store_generation(&id, &key, &archive, generation) {
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_status(trf("Could not save the encrypted profile: {}", &[&e]).into());
            });
        }
    });
}

pub fn fill_editor(ui: &AppWindow) {
    let profile = active().unwrap_or_default();
    ui.set_profile_name(profile.name.into());
    ui.set_profile_about(profile.about.into());
    ui.set_profile_message("".into());
    show_active(ui);
}

pub fn save_profile(ui: &AppWindow, name: &str, about: &str) {
    let Some(mut profile) = active() else {
        ui.set_settings_page(0);
        return;
    };
    let name = name.trim();
    if !name.is_empty() {
        profile.name = name.to_string();
    }
    profile.about = about.trim().to_string();
    if let Err(e) = profiles::save_in(&base(), &profile) {
        ui.set_profile_message(trf("Could not save: {}", &[&e]).into());
        return;
    }
    show_active(ui);
    ui.set_settings_page(0);
}

fn pick_picture(ui: &AppWindow) {
    let Some(path) = rfd::FileDialog::new()
        .set_title(tr("Choose a profile picture"))
        .add_filter(tr("Images"), &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
        .pick_file()
    else {
        return;
    };
    let target = profiles::avatar_path_in(&base(), &active_id());
    let result = image::open(&path)
        .map_err(|e| e.to_string())
        .and_then(|picture| {
            picture
                .resize_to_fill(128, 128, image::imageops::FilterType::Lanczos3)
                .save_with_format(&target, image::ImageFormat::Png)
                .map_err(|e| e.to_string())
        });
    match result {
        Ok(()) => ui.set_profile_message("".into()),
        Err(e) => ui.set_profile_message(trf("Could not use this picture: {}", &[&e]).into()),
    }
    show_active(ui);
}

fn remove_password() -> Result<(), String> {
    let profile = active().ok_or_else(|| tr("No profile is open"))?;
    if !profile.locked {
        return Ok(());
    }
    let dir = SESSION.with(|s| s.borrow().as_ref().map(|s| s.dir.clone())).ok_or_else(|| tr("No profile is open"))?;
    storage::close();
    let archive = profiles::pack(&dir)?;
    with_writer(|| profiles::remove_password_in(&base(), &profile.id, &archive))?;
    let _ = fs::remove_dir_all(&dir);
    let plain = profiles::profile_dir_in(&base(), &profile.id);
    storage::set_profile_dir(Some(plain.clone()), profile.id == profiles::DEFAULT_ID);
    SESSION.with(|s| {
        *s.borrow_mut() = Some(Session {
            id: profile.id.clone(),
            fingerprint: profiles::fingerprint(&plain),
            dir: plain,
            key: None,
        })
    });
    Ok(())
}

fn delete_active(ui: &AppWindow) {
    if ui.get_busy() {
        ui.set_profile_message(tr("Wait until the answer is finished.").into());
        return;
    }
    let id = active_id();
    let list = profiles::list_in(&base());
    let Some(next) = list.iter().find(|p| p.id != id).cloned() else {
        ui.set_profile_message(tr("The last profile cannot be deleted.").into());
        return;
    };
    let session = SESSION.with(|s| s.borrow_mut().take());
    storage::set_profile_dir(None, false);
    if let Some(session) = session
        && session.key.is_some()
    {
        let _ = fs::remove_dir_all(&session.dir);
    }
    if let Err(e) = with_writer(|| profiles::delete_in(&base(), &id)) {
        ui.set_status(trf("Could not delete the profile completely: {}", &[&e]).into());
    }
    set_active_id("");
    if next.locked {
        reload_data(ui);
        show_active(ui);
        show_unlock(ui, &next.id, "");
    } else {
        switch_to(ui, &next.id, None);
    }
}
