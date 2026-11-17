use crate::i18n::tr;
use crate::settings;
use crate::{AppWindow, Tutorial};
use slint::ComponentHandle;
use std::cell::Cell;

struct Step {
    spot: &'static str,
    page: i32,
    models_tab: i32,
    card_top: bool,
    title: &'static str,
    text: &'static str,
}

const STEPS: [Step; 21] = [
    Step { spot: "tabs", page: 0, models_tab: 0, card_top: false, title: "Chat and Models", text: "Here you switch between your chats and the model manager, where you install and test AI models." },
    Step { spot: "new-chat", page: 0, models_tab: 0, card_top: false, title: "New chat", text: "Starts an empty chat. Every chat keeps its own history, model and settings." },
    Step { spot: "search", page: 0, models_tab: 0, card_top: false, title: "Search chats", text: "Finds chats by their title and by words inside the messages." },
    Step { spot: "automations", page: 5, models_tab: 0, card_top: false, title: "Automations", text: "The model does a task on its own at set times, e.g. a short briefing every morning. Each run becomes a new chat marked with ⏰. The number shows how many are switched on." },
    Step { spot: "groups", page: 0, models_tab: 0, card_top: false, title: "Groups", text: "A group bundles chats that belong together: a project folder, its own memory, extra start arguments and searchable documents. ▦ shows all groups, + makes a new one, ↓ imports one. Right-click a chat to pin it or move it into a group." },
    Step { spot: "group", page: 0, models_tab: 0, card_top: false, title: "Group of this chat", text: "Choose the group before you write the first message, or make a new one. The chat then gets the group's folder, memory and documents." },
    Step { spot: "purpose", page: 0, models_tab: 0, card_top: false, title: "Purpose", text: "Coding: the model works on a project folder, reads ARCHITEKTUR.md first and keeps it up to date. Other: a normal chat." },
    Step { spot: "test-env", page: 0, models_tab: 0, card_top: false, title: "Test environment", text: "The model works on a copy of your project in its own folder. It may build, test and change everything there – your project stays untouched until you press “Apply to project”." },
    Step { spot: "attach", page: 0, models_tab: 0, card_top: true, title: "Add things", text: "Adds files, images, audio and video, shares a folder with the model or creates a test environment." },
    Step { spot: "mic", page: 0, models_tab: 0, card_top: true, title: "Speak", text: "Speak instead of typing. The words appear live in the text field; press again to stop." },
    Step { spot: "mic-settings", page: 0, models_tab: 0, card_top: true, title: "Microphone settings", text: "Choose the microphone, the speech model, the volume and whether live text is shown while you speak." },
    Step { spot: "think", page: 0, models_tab: 0, card_top: true, title: "Thinking", text: "Lets the model think before it answers. Auto decides by the question, On always thinks, Off answers right away." },
    Step { spot: "model", page: 0, models_tab: 0, card_top: true, title: "Model", text: "The AI model for this chat. ⇄ marks models from other servers, ☁ online models. Refresh reloads the list." },
    Step { spot: "send", page: 0, models_tab: 0, card_top: true, title: "Send", text: "Sends the message – Enter does the same. Shift+Enter makes a new line. While the model writes, this button stops it." },
    Step { spot: "activity", page: 0, models_tab: 0, card_top: false, title: "Activity and preview", text: "Opens the side panel: what the model is doing right now (reading files, searching, running commands) and a preview of files." },
    Step { spot: "models-tabs", page: 1, models_tab: 0, card_top: false, title: "Installed models", text: "Your models: test how fast they run, see their size and remove them." },
    Step { spot: "models-tabs", page: 1, models_tab: 1, card_top: false, title: "Available models", text: "Download new models. The list shows which ones fit your graphics card." },
    Step { spot: "models-tabs", page: 1, models_tab: 2, card_top: false, title: "Online models", text: "Use Claude Code, Codex, Gemini or Grok through their command-line tools: install, log in, activate." },
    Step { spot: "models-tabs", page: 1, models_tab: 3, card_top: false, title: "Servers", text: "Connect LM Studio, llama.cpp, vLLM or another OpenAI-compatible server. Its models then appear in the model list with ⇄." },
    Step { spot: "ollama", page: 0, models_tab: 0, card_top: false, title: "Ollama", text: "Ollama runs the local models. Start or stop it here – or install it if it is missing." },
    Step { spot: "profile", page: 0, models_tab: 0, card_top: false, title: "Profile and settings", text: "Your profile and the menu: settings (language, this tour, reset), Ollama settings, tools and start arguments. Have fun!" },
];

thread_local! {
    static INDEX: Cell<usize> = const { Cell::new(0) };
}

pub fn setup(ui: &AppWindow) {
    let current = settings::load();
    ui.set_ui_language_index(if crate::i18n::language() == "de" { 1 } else { 0 });
    if !current.welcome_seen {
        start(ui);
    }

    let state = ui.global::<Tutorial>();
    let weak = ui.as_weak();
    state.on_choose_language(move |code| {
        if let Some(ui) = weak.upgrade() {
            apply_language(&ui, &code);
        }
    });

    let weak = ui.as_weak();
    state.on_next(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<Tutorial>();
            if state.get_language_step() {
                state.set_language_step(false);
                show(&ui, 0);
            } else {
                let next = INDEX.with(Cell::get) + 1;
                if next >= STEPS.len() {
                    finish(&ui);
                } else {
                    show(&ui, next);
                }
            }
        }
    });

    let weak = ui.as_weak();
    state.on_back(move || {
        if let Some(ui) = weak.upgrade() {
            let index = INDEX.with(Cell::get);
            if index == 0 {
                ui.global::<Tutorial>().set_language_step(true);
            } else {
                show(&ui, index - 1);
            }
        }
    });

    let weak = ui.as_weak();
    state.on_skip(move || {
        if let Some(ui) = weak.upgrade() {
            finish(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_start_tour(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_settings_page(0);
            start(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_ui_language_changed(move |index| {
        if let Some(ui) = weak.upgrade() {
            apply_language(&ui, if index == 1 { "de" } else { "en" });
        }
    });
}

fn apply_language(ui: &AppWindow, code: &str) {
    crate::i18n::set_language(code);
    settings::update(|s| s.ui_language = code.to_string());
    ui.global::<Tutorial>().set_language(code.into());
    ui.set_ui_language_index(if code == "de" { 1 } else { 0 });
    crate::i18n::refresh(ui);
}

fn start(ui: &AppWindow) {
    let state = ui.global::<Tutorial>();
    state.set_language(crate::i18n::language().into());
    state.set_count(STEPS.len() as i32);
    state.set_language_step(true);
    state.set_active(true);
    ui.set_page(0);
    ui.invoke_new_chat();
}

fn show(ui: &AppWindow, index: usize) {
    let step = &STEPS[index];
    INDEX.with(|slot| slot.set(index));
    let state = ui.global::<Tutorial>();
    ui.set_page(step.page);
    state.set_models_tab(step.models_tab);
    state.set_card_top(step.card_top);
    state.set_title(tr(step.title).into());
    state.set_text(tr(step.text).into());
    state.set_step(index as i32 + 1);
    state.set_spot(step.spot.into());
}

fn finish(ui: &AppWindow) {
    let state = ui.global::<Tutorial>();
    state.set_active(false);
    state.set_language_step(false);
    state.set_spot("".into());
    ui.set_page(0);
    settings::update(|s| s.welcome_seen = true);
}
