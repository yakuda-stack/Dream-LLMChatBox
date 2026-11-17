use crate::i18n::{tr, trf};
use crate::ollama;
use crate::server;
use crate::settings;
use crate::AppWindow;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::Rc;

const CONTEXTS: [(u32, &str); 6] = [
    (4096, "4K"),
    (8192, "8K"),
    (16384, "16K"),
    (32768, "32K"),
    (65536, "64K (default)"),
    (131072, "128K"),
];

const KEEP_ALIVE: [(&str, &str); 5] = [
    ("0", "Unload right after answering"),
    ("5m", "5 minutes (default)"),
    ("30m", "30 minutes"),
    ("1h", "1 hour"),
    ("-1", "Keep loaded"),
];

pub const ANSWER_LANGUAGES: [(&str, &str, &str); 12] = [
    ("", "Automatic (like you write)", ""),
    ("de", "Deutsch", "German"),
    ("en", "English", "English"),
    ("fr", "Français", "French"),
    ("es", "Español", "Spanish"),
    ("it", "Italiano", "Italian"),
    ("nl", "Nederlands", "Dutch"),
    ("pl", "Polski", "Polish"),
    ("pt", "Português", "Portuguese"),
    ("tr", "Türkçe", "Turkish"),
    ("ru", "Русский", "Russian"),
    ("ja", "日本語", "Japanese"),
];

pub fn answer_language_name(code: &str) -> Option<&'static str> {
    ANSWER_LANGUAGES
        .iter()
        .find(|(c, _, name)| *c == code && !name.is_empty())
        .map(|(_, _, name)| *name)
}

pub fn set_option_labels(ui: &AppWindow) {
    let contexts: Vec<String> = CONTEXTS.iter().map(|(_, label)| tr(label)).collect();
    let keep_alive: Vec<String> = KEEP_ALIVE.iter().map(|(_, label)| tr(label)).collect();
    let languages: Vec<String> = ANSWER_LANGUAGES.iter().map(|(_, label, _)| tr(label)).collect();
    ui.set_context_options(strings(contexts.iter().map(String::as_str)));
    ui.set_keep_alive_options(strings(keep_alive.iter().map(String::as_str)));
    ui.set_language_options(strings(languages.iter().map(String::as_str)));
}

pub fn setup(ui: &AppWindow) {
    let current = settings::load();
    ollama::set_base_url(&current.ollama_url);
    set_option_labels(ui);
    ui.set_show_context_bar(current.show_context_bar);

    let weak = ui.as_weak();
    ui.on_pick_work_dir(move || {
        let Some(ui) = weak.upgrade() else { return };
        if let Some(path) = rfd::FileDialog::new()
            .set_directory(crate::workspace::base())
            .pick_folder()
        {
            ui.set_work_dir(path.to_string_lossy().to_string().into());
        }
    });

    let weak = ui.as_weak();
    ui.on_open_settings(move |page| {
        if let Some(ui) = weak.upgrade() {
            let current = settings::load();
            crate::profiles_ui::fill_editor(&ui);
            ui.set_ollama_url(current.ollama_url.into());
            ui.set_context_index(
                CONTEXTS
                    .iter()
                    .position(|(value, _)| *value == current.context_length)
                    .unwrap_or(4) as i32,
            );
            ui.set_keep_alive_index(
                KEEP_ALIVE
                    .iter()
                    .position(|(value, _)| *value == current.keep_alive)
                    .unwrap_or(1) as i32,
            );
            ui.set_system_prompt_text(current.system_prompt.into());
            ui.set_data_info(
                trf(
                    "{} chats stored  ·  {} MB",
                    &[
                        &crate::storage::count_chats(),
                        &format!("{:.1}", crate::storage::data_size() as f64 / 1_000_000.0),
                    ],
                )
                .into(),
            );
            ui.set_language_index(
                ANSWER_LANGUAGES
                    .iter()
                    .position(|(code, _, _)| *code == current.answer_language)
                    .unwrap_or(0) as i32,
            );
            ui.set_tools_enabled(current.tools_enabled);
            ui.set_auto_switch(current.auto_switch);
            ui.set_auto_continue(current.auto_continue);
            ui.set_show_context_bar(current.show_context_bar);
            ui.set_auto_compact(current.auto_compact);
            ui.set_compact_at(current.compact_at as i32);
            ui.set_work_dir(current.work_dir.as_str().into());
            ui.set_container_index(crate::container::engine_index(&current.container_engine));
            ui.set_backend_index(current.ollama_backend as i32);
            ui.set_backend_status(crate::ollama_backend::status().into());
            ui.set_container_image(current.container_image.as_str().into());
            ui.set_container_status(crate::container::status(&current.container_engine).into());
            ui.set_embed_model(current.embed_model.as_str().into());
            ui.set_settings_page(page);
        }
    });

    let weak = ui.as_weak();
    ui.on_save_profile(move |name, about| {
        if let Some(ui) = weak.upgrade() {
            crate::profiles_ui::save_profile(&ui, &name, &about);
        }
    });

    let weak = ui.as_weak();
    ui.on_save_general(move |language_index, tools_enabled| {
        if let Some(ui) = weak.upgrade() {
            let code = ANSWER_LANGUAGES
                .get(language_index as usize)
                .map(|(code, _, _)| code.to_string())
                .unwrap_or_default();
            settings::update(|s| {
                s.answer_language = code;
                s.tools_enabled = tools_enabled;
                s.auto_switch = ui.get_auto_switch();
                s.auto_continue = ui.get_auto_continue();
                s.show_context_bar = ui.get_show_context_bar();
                s.auto_compact = ui.get_auto_compact();
                s.compact_at = ui.get_compact_at().clamp(30, 95) as u32;
                s.work_dir = ui.get_work_dir().trim().to_string();
                s.container_engine = crate::container::engine_name(ui.get_container_index());
                s.container_image = ui.get_container_image().trim().to_string();
                s.embed_model = ui.get_embed_model().trim().to_string();
            });
            ui.set_settings_page(0);
        }
    });

    ui.set_kill_command(server::kill_command().into());
    let weak = ui.as_weak();
    ui.on_kill_in_terminal(move || {
        if let Some(ui) = weak.upgrade() {
            let script = format!(
                "{}; echo; echo 'Done. Press Enter to close.'; read _",
                ui.get_kill_command()
            );
            if let Err(e) = crate::terminal::run(&script) {
                ui.set_status(e.into());
            }
        }
    });

    ui.set_ollama_installed(server::is_installed());
    let weak = ui.as_weak();
    ui.on_backend_apply(move |index| {
        if let Some(ui) = weak.upgrade() {
            ui.set_backend_busy(true);
            let weak = ui.as_weak();
            std::thread::spawn(move || {
                let result = crate::ollama_backend::apply(index.max(0) as u32);
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.set_backend_busy(false);
                    let text = match result {
                        Ok(done) => done,
                        Err(e) => e,
                    };
                    ui.set_backend_status(text.into());
                    ui.set_ollama_running(ollama::is_running());
                });
            });
        }
    });

    let weak = ui.as_weak();
    ui.on_container_changed(move |index| {
        if let Some(ui) = weak.upgrade() {
            let engine = crate::container::engine_name(index);
            ui.set_container_status(crate::container::status(&engine).into());
        }
    });

    let weak = ui.as_weak();
    ui.on_ollama_install(move || {
        if let Some(ui) = weak.upgrade() {
            install_ollama(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_ollama_toggle(move || {
        if let Some(ui) = weak.upgrade() {
            toggle_server(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_save_ollama(move |url, context_index, keep_index, prompt| {
        if let Some(ui) = weak.upgrade() {
            let context = CONTEXTS
                .get(context_index as usize)
                .map(|(value, _)| *value)
                .unwrap_or(65536);
            let keep = KEEP_ALIVE
                .get(keep_index as usize)
                .map(|(value, _)| value.to_string())
                .unwrap_or_else(|| "5m".to_string());
            let url = url.trim().to_string();
            settings::update(|s| {
                s.ollama_url = url.clone();
                s.context_length = context;
                s.model_context.clear();
                s.keep_alive = keep;
                s.system_prompt = prompt.trim().to_string();
            });
            ollama::set_base_url(&url);
            ui.set_settings_page(0);
            ui.invoke_refresh_models();
        }
    });
}

fn install_ollama(ui: &AppWindow) {
    let script = format!(
        "{}; echo; echo 'Done – you can close this window.'; read -r _",
        server::INSTALL_SCRIPT
    );
    let child = match crate::terminal::run(&script) {
        Ok(child) => child,
        Err(e) => {
            ui.set_status(trf("{}. Run it yourself: {}", &[&e, &server::INSTALL_SCRIPT]).into());
            return;
        }
    };
    ui.set_ollama_switching(true);
    ui.set_status(tr("Installing Ollama in a terminal window…").into());
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
        let mut installed = server::is_installed();
        for _ in 0..24 {
            if installed {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(5));
            installed = server::is_installed();
        }
        let running = ollama::is_running();
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.set_ollama_switching(false);
            ui.set_ollama_installed(installed || running);
            ui.set_ollama_running(running);
            if running {
                ui.invoke_refresh_models();
            }
            ui.set_status(if installed { tr("Ollama is installed.") } else { tr("Ollama was not installed.") }.into());
        });
    });
}

fn toggle_server(ui: &AppWindow) {
    if ui.get_ollama_switching() {
        return;
    }
    let running = ui.get_ollama_running();
    ui.set_ollama_switching(true);
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let result = if running { server::stop() } else { server::start() };
        let now_running = ollama::is_running();
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.set_ollama_switching(false);
            ui.set_ollama_running(now_running);
            match result {
                Ok(()) if now_running => ui.invoke_refresh_models(),
                Ok(()) => {
                    ui.set_models(ModelRc::default());
                    ui.set_status(tr("Ollama stopped.").into());
                }
                Err(e) => ui.set_status(format!("Ollama: {e}").into()),
            }
        });
    });
}

pub fn display_name(name: &str) -> String {
    if name.trim().is_empty() {
        tr("You")
    } else {
        name.trim().to_string()
    }
}

fn strings<'a>(items: impl Iterator<Item = &'a str>) -> ModelRc<SharedString> {
    let list: Vec<SharedString> = items.map(SharedString::from).collect();
    ModelRc::new(Rc::new(VecModel::from(list)))
}
