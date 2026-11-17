use crate::storage;
use crate::{ActivityEntry, AppWindow};
use serde::{Deserialize, Serialize};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};

pub const RUNNING: i32 = 0;
pub const DONE: i32 = 1;
pub const FAILED: i32 = 2;
pub const STOPPED: i32 = 3;
const MAX_ENTRIES: usize = 300;

static NEXT_ID: AtomicI32 = AtomicI32::new(1);

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
struct Step {
    id: i32,
    icon: String,
    text: String,
    #[serde(default)]
    detail: String,
    state: i32,
    time: String,
}

thread_local! {
    static CHATS: RefCell<HashMap<String, Vec<Step>>> = RefCell::new(HashMap::new());
}

pub fn setup(ui: &AppWindow) {
    show(ui);
    let weak = ui.as_weak();
    ui.on_clear_activity(move || {
        let Some(ui) = weak.upgrade() else { return };
        let chat = ui.get_current_chat_id().to_string();
        change(&chat, |steps| steps.retain(|step| step.state == RUNNING));
        show(&ui);
    });
}

pub fn clear() {
    CHATS.with(|chats| chats.borrow_mut().clear());
}

fn steps_of(chat: &str) -> Vec<Step> {
    CHATS.with(|chats| {
        let mut chats = chats.borrow_mut();
        chats
            .entry(chat.to_string())
            .or_insert_with(|| {
                if chat.is_empty() {
                    Vec::new()
                } else {
                    let mut steps: Vec<Step> =
                        serde_json::from_str(&storage::load_activity(chat)).unwrap_or_default();
                    for step in steps.iter_mut().filter(|step| step.state == RUNNING) {
                        step.state = STOPPED;
                    }
                    steps
                }
            })
            .clone()
    })
}

fn change(chat: &str, edit: impl FnOnce(&mut Vec<Step>)) {
    let mut steps = steps_of(chat);
    edit(&mut steps);
    steps.truncate(MAX_ENTRIES);
    if !chat.is_empty()
        && let Ok(json) = serde_json::to_string(&steps)
    {
        let _ = storage::save_activity(chat, &json);
    }
    CHATS.with(|chats| chats.borrow_mut().insert(chat.to_string(), steps));
}

pub fn show(ui: &AppWindow) {
    let chat = ui.get_current_chat_id().to_string();
    let entries: Vec<ActivityEntry> = steps_of(&chat)
        .into_iter()
        .map(|step| ActivityEntry {
            id: step.id,
            icon: step.icon.into(),
            text: step.text.into(),
            detail: step.detail.into(),
            state: step.state,
            time: step.time.into(),
        })
        .collect();
    ui.set_activity(ModelRc::new(VecModel::from(entries)));
    let running = CHATS.with(|chats| {
        chats
            .borrow()
            .values()
            .flat_map(|steps| steps.iter())
            .find(|step| step.state == RUNNING)
            .map(|step| step.text.clone())
            .unwrap_or_default()
    });
    ui.set_activity_text(running.into());
}

pub fn start(weak: &slint::Weak<AppWindow>, chat: &str, icon: &str, text: &str) -> i32 {
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let step = Step {
        id,
        icon: icon.to_string(),
        text: text.to_string(),
        detail: String::new(),
        state: RUNNING,
        time: chrono::Local::now().format("%H:%M:%S").to_string(),
    };
    let chat = chat.to_string();
    let _ = weak.upgrade_in_event_loop(move |ui| {
        change(&chat, |steps| steps.insert(0, step));
        show(&ui);
    });
    id
}

fn edit(weak: &slint::Weak<AppWindow>, chat: &str, id: i32, update: impl FnOnce(&mut Step) + Send + 'static) {
    let chat = chat.to_string();
    let _ = weak.upgrade_in_event_loop(move |ui| {
        change(&chat, |steps| {
            if let Some(step) = steps.iter_mut().find(|step| step.id == id) {
                update(step);
            }
        });
        show(&ui);
    });
}

pub fn rename(weak: &slint::Weak<AppWindow>, chat: &str, id: i32, text: &str) {
    let text = text.to_string();
    edit(weak, chat, id, move |step| step.text = text);
}

pub fn finish(weak: &slint::Weak<AppWindow>, chat: &str, id: i32, state: i32, detail: &str) {
    let detail: String = detail.chars().take(300).collect();
    edit(weak, chat, id, move |step| {
        step.state = state;
        step.detail = detail;
    });
}
