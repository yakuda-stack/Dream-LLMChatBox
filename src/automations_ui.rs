use crate::i18n::{tr, trf};
use crate::automations::{self, Automation};
use crate::cloud_cli;
use crate::context_saver;
use crate::groups;
use crate::ollama::{self, ChatConfig, Message};
use crate::settings;
use crate::storage::{self, ChatFile, StoredMessage};
use crate::{AppWindow, AutomationEntry, Automations};
use chrono::{Datelike, Local};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

static RUNNING: AtomicBool = AtomicBool::new(false);
const CHECK_EVERY: Duration = Duration::from_secs(30);

thread_local! {
    static TICKER: Timer = Timer::default();
    static MODELS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static GROUPS: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn strings(items: &[String]) -> ModelRc<SharedString> {
    ModelRc::new(Rc::new(VecModel::from(
        items.iter().map(|item| SharedString::from(item.as_str())).collect::<Vec<_>>(),
    )))
}

pub fn set_labels(ui: &AppWindow) {
    let state = ui.global::<Automations>();
    let labels = |items: &[&str]| strings(&items.iter().map(|item| tr(item)).collect::<Vec<_>>());
    state.set_kinds(labels(&automations::KINDS));
    state.set_weekdays(labels(&automations::WEEKDAYS));
}

pub fn setup(ui: &AppWindow) {
    let state = ui.global::<Automations>();
    set_labels(ui);
    refresh(ui);

    let weak = ui.as_weak();
    state.on_create(move || {
        if let Some(ui) = weak.upgrade() {
            fill_form(&ui, &Automation { time: "08:00".into(), hours: 6, enabled: true, ..Default::default() });
            ui.global::<Automations>().set_editing(-1);
        }
    });

    let weak = ui.as_weak();
    state.on_edit(move |index| {
        if let Some(ui) = weak.upgrade()
            && let Some(automation) = automations::load().get(index as usize)
        {
            fill_form(&ui, automation);
            ui.global::<Automations>().set_editing(index);
        }
    });

    let weak = ui.as_weak();
    state.on_cancel(move || {
        if let Some(ui) = weak.upgrade() {
            let state = ui.global::<Automations>();
            state.set_editing(-2);
            state.set_message("".into());
        }
    });

    let weak = ui.as_weak();
    state.on_save(move || {
        if let Some(ui) = weak.upgrade() {
            save_form(&ui);
        }
    });

    let weak = ui.as_weak();
    state.on_remove(move |index| {
        if let Some(ui) = weak.upgrade() {
            let mut list = automations::load();
            if (index as usize) < list.len() {
                list.remove(index as usize);
                store(&ui, &list);
            }
        }
    });

    let weak = ui.as_weak();
    state.on_toggle(move |index, on| {
        if let Some(ui) = weak.upgrade() {
            let mut list = automations::load();
            if let Some(automation) = list.get_mut(index as usize) {
                automation.enabled = on;
                if on {
                    automation.last_run = Local::now().timestamp();
                }
            }
            store(&ui, &list);
        }
    });

    let weak = ui.as_weak();
    state.on_run_now(move |index| {
        if let Some(ui) = weak.upgrade()
            && let Some(automation) = automations::load().get(index as usize)
        {
            run(&ui, automation.clone());
        }
    });

    let weak = ui.as_weak();
    state.on_open_result(move |index| {
        if let Some(ui) = weak.upgrade()
            && let Some(automation) = automations::load().get(index as usize)
        {
            ui.set_page(0);
            ui.invoke_load_chat(automation.last_chat.as_str().into());
        }
    });

    let weak = ui.as_weak();
    TICKER.with(|timer| {
        timer.start(TimerMode::Repeated, CHECK_EVERY, move || {
            if let Some(ui) = weak.upgrade() {
                tick(&ui);
            }
        });
    });
}

fn store(ui: &AppWindow, list: &[Automation]) {
    if let Err(e) = automations::save(list) {
        ui.global::<Automations>().set_message(trf("Could not save: {}", &[&e]).into());
    }
    refresh(ui);
}

pub fn refresh(ui: &AppWindow) {
    let all = automations::load();
    let active = all.iter().filter(|automation| automation.enabled).count() as i32;
    let entries: Vec<AutomationEntry> = all
        .iter()
        .map(|automation| {
            let next = automations::next_run(automation)
                .filter(|_| automation.enabled)
                .map(|when| {
                    let day = automations::WEEKDAYS[when.weekday().num_days_from_monday() as usize];
                    let date = format!("{} {}", tr(day), when.format("%d.%m. %H:%M"));
                    trf("next {}", &[&date])
                });
            AutomationEntry {
                name: automation.name.as_str().into(),
                schedule: automations::schedule_text(automation).into(),
                detail: next.unwrap_or_else(|| if automation.enabled { String::new() } else { tr("off") }).into(),
                enabled: automation.enabled,
                has_result: !automation.last_chat.is_empty(),
            }
        })
        .collect();
    let state = ui.global::<Automations>();
    state.set_entries(ModelRc::new(Rc::new(VecModel::from(entries))));
    state.set_running(RUNNING.load(Ordering::SeqCst));
    state.set_active_count(active);
}

fn fill_form(ui: &AppWindow, automation: &Automation) {
    let mut models = vec![tr("Model selected in the chat")];
    models.extend(ui.get_models().iter().map(|model| model.to_string()));
    let all_groups = groups::list();
    let mut group_names = vec![tr("No group")];
    group_names.extend(all_groups.iter().map(|group| group.name.clone()));
    let state = ui.global::<Automations>();
    state.set_models(strings(&models));
    state.set_groups(strings(&group_names));
    state.set_form_model(models.iter().position(|model| *model == automation.model).filter(|_| !automation.model.is_empty()).unwrap_or(0) as i32);
    state.set_form_group(all_groups.iter().position(|group| group.id == automation.group).map_or(0, |index| index + 1) as i32);
    state.set_form_name(automation.name.as_str().into());
    state.set_form_prompt(automation.prompt.as_str().into());
    state.set_form_kind(automation.kind as i32);
    state.set_form_time(automation.time.as_str().into());
    state.set_form_weekday(automation.weekday as i32);
    state.set_form_hours(automation.hours.max(1) as i32);
    state.set_message("".into());
    MODELS.with(|slot| *slot.borrow_mut() = models);
    GROUPS.with(|slot| *slot.borrow_mut() = all_groups.into_iter().map(|group| group.id).collect());
}

fn save_form(ui: &AppWindow) {
    let state = ui.global::<Automations>();
    let editing = state.get_editing();
    let mut list = automations::load();
    let mut automation = list
        .get(editing as usize)
        .filter(|_| editing >= 0)
        .cloned()
        .unwrap_or_else(|| Automation {
            id: storage::new_id(),
            enabled: true,
            created: Local::now().timestamp(),
            ..Default::default()
        });
    automation.name = state.get_form_name().trim().to_string();
    automation.prompt = state.get_form_prompt().trim().to_string();
    automation.kind = state.get_form_kind().max(0) as u32;
    automation.time = state.get_form_time().trim().replace('.', ":");
    automation.weekday = state.get_form_weekday().max(0) as u32;
    automation.hours = state.get_form_hours().max(1) as u32;
    let model_index = state.get_form_model().max(0) as usize;
    automation.model = if model_index == 0 {
        String::new()
    } else {
        MODELS.with(|slot| slot.borrow().get(model_index).cloned().unwrap_or_default())
    };
    let group_index = state.get_form_group().max(0) as usize;
    automation.group = if group_index == 0 {
        String::new()
    } else {
        GROUPS.with(|slot| slot.borrow().get(group_index - 1).cloned().unwrap_or_default())
    };
    if let Err(e) = automations::check(&automation) {
        state.set_message(e.into());
        return;
    }
    if editing >= 0 && (editing as usize) < list.len() {
        automation.last_run = Local::now().timestamp();
        list[editing as usize] = automation;
    } else {
        list.push(automation);
    }
    state.set_editing(-2);
    state.set_message("".into());
    store(ui, &list);
}

fn tick(ui: &AppWindow) {
    refresh(ui);
    if RUNNING.load(Ordering::SeqCst) {
        return;
    }
    let mut list = automations::load();
    let now = Local::now();
    let Some(index) = list.iter().position(|automation| automations::is_due(automation, now)) else {
        return;
    };
    list[index].last_run = now.timestamp();
    let automation = list[index].clone();
    let _ = automations::save(&list);
    run(ui, automation);
}

fn answer(model: &str, messages: &[Message]) -> Result<String, String> {
    let current = settings::load();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut text = String::new();
    let collect = |chunk: &str, thinking: bool| {
        if !thinking {
            text.push_str(chunk);
        }
    };
    if let Some(provider) = cloud_cli::from_model(model) {
        cloud_cli::chat(provider, messages, &[], &cancel, collect)?;
    } else if let Some((server, id)) = crate::openai_compat::from_model(model, &current.servers) {
        crate::openai_compat::chat_stream(&server, &id, messages, None, &cancel, collect)?;
    } else {
        let config = ChatConfig {
            context_length: current.context_for(model),
            keep_alive: current.keep_alive.clone(),
            think: None,
        };
        ollama::chat_stream(model, messages, &config, None, &cancel, collect)?;
    }
    Ok(context_saver::strip_thinking(&text).trim().to_string())
}

fn run(ui: &AppWindow, automation: Automation) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let model = if automation.model.is_empty() {
        ui.get_selected_model().to_string()
    } else {
        automation.model.clone()
    };
    refresh(ui);
    if model.is_empty() {
        RUNNING.store(false, Ordering::SeqCst);
        ui.set_status(trf("⏰ {}: no model selected", &[&automation.name]).into());
        refresh(ui);
        return;
    }
    let mut system = crate::chat::system_message();
    system.content.push_str("\n\n");
    system.content.push_str(&automations::run_note(&automation.name));
    let messages = vec![
        system,
        Message { role: "user".to_string(), content: automation.prompt.clone(), ..Default::default() },
    ];
    ui.set_status(trf("⏰ {} is running…", &[&automation.name]).into());
    let weak = ui.as_weak();
    thread::spawn(move || {
        let result = answer(&model, &messages);
        let _ = weak.upgrade_in_event_loop(move |ui| {
            RUNNING.store(false, Ordering::SeqCst);
            let (content, status) = match &result {
                Ok(text) if !text.is_empty() => (text.clone(), String::new()),
                Ok(_) => ("[Error: the model gave an empty answer]".to_string(), "error".to_string()),
                Err(e) => (format!("[Error: {e}]"), "error".to_string()),
            };
            let id = storage::new_id();
            let chat = ChatFile {
                id: id.clone(),
                title: format!("⏰ {} · {}", automation.name, Local::now().format("%d.%m. %H:%M")),
                model: model.clone(),
                messages: vec![
                    StoredMessage {
                        role: "user".to_string(),
                        content: automation.prompt.clone(),
                        attachments: String::new(),
                        status: String::new(),
                        info: format!("⏰ {}", automation.name),
                    },
                    StoredMessage {
                        role: "assistant".to_string(),
                        content,
                        attachments: String::new(),
                        status: status.clone(),
                        info: String::new(),
                    },
                ],
                folders: String::new(),
                group: automation.group.clone(),
                mode: String::new(),
            };
            if let Err(e) = storage::save_chat(&chat) {
                ui.set_status(trf("⏰ {}: could not save the result: {}", &[&automation.name, &e]).into());
                refresh(&ui);
                return;
            }
            let mut list = automations::load();
            if let Some(stored) = list.iter_mut().find(|stored| stored.id == automation.id) {
                stored.last_chat = id;
            }
            let _ = automations::save(&list);
            crate::chat::refresh_chat_list(&ui);
            refresh(&ui);
            let done = if status.is_empty() { tr("is done") } else { tr("failed") };
            let text = trf("⏰ {} {} – the result is a new chat.", &[&automation.name, &done]);
            ui.set_status(text.as_str().into());
            notify(&automation.name, &done);
        });
    });
}

fn notify(name: &str, done: &str) {
    let _ = std::process::Command::new("notify-send")
        .args(["--app-name=Dream-LLMChatBox", "--icon=dream-llmchatbox"])
        .arg(format!("⏰ {name} {done}"))
        .arg(tr("The result is a new chat in Dream-LLMChatBox."))
        .spawn();
}
