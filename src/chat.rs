use crate::i18n::{tr, trf};
use crate::activity;
use crate::approval;
use crate::attachments;
use crate::auto_switch;
use crate::coding;
use crate::coding_ui;
use crate::checkpoint;
use crate::auto_check;
use crate::cloud_cli;
use crate::composer;
use crate::context_saver;
use crate::file_tools;
use crate::folders;
use crate::groups;
use crate::groups_ui;
use crate::markdown;
use crate::nudge;
use crate::models_page;
use crate::ollama::{self, ChatConfig, ChatStats, Message};
use crate::sandbox;
use crate::settings;
use crate::start_args;
use crate::storage::{self, ChatFile, StoredMessage};
use crate::tools;
use crate::workspace;
use crate::{AppWindow, ChatEntry, ChatMessage, MessageSegment};
use slint::{ComponentHandle, Model, ModelRc, SharedString, Timer, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const MIN_CONTEXT: u32 = 4096;

const CONTINUE_PROMPT: &str =
    "Continue exactly where you stopped. Do not repeat anything you already wrote.";

pub fn setup(ui: &AppWindow) {
    set_messages(ui, Vec::new());
    refresh_chat_list(ui);
    let cancel = Arc::new(AtomicBool::new(false));

    ui.on_model_changed(|model| {
        if !model.is_empty() {
            settings::update(|s| s.last_model = model.to_string());
        }
    });

    let weak = ui.as_weak();
    ui.on_refresh_models(move || load_models(weak.clone()));

    let weak = ui.as_weak();
    ui.on_new_chat(move || {
        if let Some(ui) = weak.upgrade() {
            set_messages(&ui, Vec::new());
            ui.set_current_chat_id("".into());
            ui.set_auto_sync(false);
            folders::set(&ui, &[]);
            groups_ui::set_current(&ui, "");
            coding_ui::reset(&ui);
            ui.set_think_mode(auto_switch::THINK_AUTO);
            ui.set_work_changes(0);
            crate::preview::chat_changed(&ui);
            update_chat_title(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_undo_changes(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        if ui.get_busy() {
            return;
        }
        let chat = ui.get_current_chat_id().to_string();
        match checkpoint::undo(&checkpoint::dir(&chat, index as usize)) {
            Ok(count) => ui.set_status(
                trf(
                    "Undone: {} {} back as before this answer",
                    &[&count, &if count == 1 { tr("file is") } else { tr("files are") }],
                )
                .into(),
            ),
            Err(e) => ui.set_status(trf("Could not undo everything: {}", &[&e]).into()),
        }
        mark_checkpoints(&ui);
        crate::work_ui::refresh(&ui);
        crate::preview::refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_think_changed(move |mode| {
        if let Some(ui) = weak.upgrade() {
            let id = ui.get_current_chat_id();
            if !id.is_empty() {
                let _ = storage::set_chat_think(&id, mode);
            }
            let text = match mode {
                auto_switch::THINK_ON => tr("Thinking on for this chat"),
                auto_switch::THINK_OFF => tr("Thinking off for this chat – faster answers"),
                _ => tr("Thinking auto: off for simple chat, on for coding and complex tasks"),
            };
            ui.set_status(text.into());
        }
    });

    let weak = ui.as_weak();
    ui.on_chat_page(move |offset| {
        if let Some(ui) = weak.upgrade() {
            ui.set_chat_offset(offset.max(0));
            refresh_chat_list(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_load_chat(move |id| {
        if let Some(ui) = weak.upgrade() {
            open_chat(&ui, &id);
        }
    });

    let weak = ui.as_weak();
    ui.on_delete_all_chats(move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_busy() {
                return;
            }
            match storage::delete_all_chats() {
                Ok(()) => {
                    set_messages(&ui, Vec::new());
                    ui.set_current_chat_id("".into());
                    folders::set(&ui, &[]);
                    coding_ui::reset(&ui);
                    ui.set_chat_offset(0);
                    refresh_chat_list(&ui);
                    ui.set_settings_page(0);
                    ui.set_status(tr("All chats were deleted.").into());
                }
                Err(e) => ui.set_status(trf("Could not delete chats: {}", &[&e]).into()),
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_rename_chat(move |id, title| {
        if let Some(ui) = weak.upgrade() {
            rename_chat(&ui, &id, &title);
        }
    });

    let weak = ui.as_weak();
    ui.on_delete_chat(move |id| {
        if let Some(ui) = weak.upgrade() {
            delete_chat(&ui, &id);
        }
    });

    let weak = ui.as_weak();
    let send_cancel = cancel.clone();
    ui.on_send(move |text| send_message(weak.clone(), text, send_cancel.clone()));

    let weak = ui.as_weak();
    let continue_cancel = cancel.clone();
    ui.on_continue_reply(move || {
        if let Some(ui) = weak.upgrade() {
            continue_reply(&ui, continue_cancel.clone());
        }
    });

    let weak = ui.as_weak();
    ui.on_edit_prompt(move |index| {
        if let Some(ui) = weak.upgrade() {
            edit_prompt(&ui, index as usize);
        }
    });

    ui.on_stop(move || cancel.store(true, Ordering::SeqCst));

    let weak = ui.as_weak();
    ui.on_compact_chat(move || {
        if let Some(ui) = weak.upgrade() {
            crate::compact::start(&ui, false);
        }
    });

    load_models(ui.as_weak());
}

fn segments(text: &str) -> Vec<MessageSegment> {
    markdown::split(text)
        .into_iter()
        .map(|segment| MessageSegment {
            is_code: segment.kind == markdown::Kind::Code,
            is_heading: segment.kind == markdown::Kind::Heading,
            is_tool: segment.kind == markdown::Kind::Tool,
            is_think: segment.kind == markdown::Kind::Think,
            is_bullet: segment.kind == markdown::Kind::Bullet,
            level: if segment.kind == markdown::Kind::Bullet {
                segment.language.parse().unwrap_or(0)
            } else {
                0
            },
            language: segment.language.into(),
            text: segment.text.into(),
        })
        .collect()
}

pub fn chat_message(
    is_user: bool,
    text: &str,
    attachments_json: &str,
    status: &str,
    info: &str,
) -> ChatMessage {
    let segments = segments(text);
    ChatMessage {
        is_user,
        text: text.into(),
        attachment_names: attachments::names(&attachments::from_json(attachments_json)).into(),
        attachments_json: attachments_json.into(),
        segments: ModelRc::new(Rc::new(VecModel::from(segments))),
        status: status.into(),
        info: info.into(),
        undo_count: 0,
    }
}

pub fn mark_checkpoints(ui: &AppWindow) {
    let chat = ui.get_current_chat_id().to_string();
    let list = message_list();
    let latest = if chat.is_empty() { None } else { checkpoint::latest(&chat, list.row_count()) };
    for index in 0..list.row_count() {
        let Some(mut message) = list.row_data(index) else { continue };
        let count = match latest {
            Some((answer, files)) if answer == index && !message.is_user => files as i32,
            _ => 0,
        };
        if message.undo_count != count {
            message.undo_count = count;
            list.set_row_data(index, message);
        }
    }
}

thread_local! {
    static MESSAGES: RefCell<Rc<VecModel<ChatMessage>>> = RefCell::new(Rc::new(VecModel::default()));
}

fn set_messages(ui: &AppWindow, messages: Vec<ChatMessage>) {
    let list = Rc::new(VecModel::from(messages));
    MESSAGES.with(|m| *m.borrow_mut() = list.clone());
    ui.set_messages(ModelRc::from(list));
    update_context_bar(ui);
}

pub(crate) fn message_list() -> Rc<VecModel<ChatMessage>> {
    MESSAGES.with(|m| m.borrow().clone())
}

pub fn refresh_chat_list(ui: &AppWindow) {
    crate::chat_list::load_pinned();
    groups_ui::refresh(ui);
    let total = storage::count_ungrouped();
    let mut offset = ui.get_chat_offset().max(0) as usize;
    if offset >= total {
        offset = total.saturating_sub(1) / storage::PAGE_SIZE * storage::PAGE_SIZE;
    }
    ui.set_chat_offset(offset as i32);
    ui.set_chat_total(total as i32);
    ui.set_chat_page_size(storage::PAGE_SIZE as i32);

    let entries: Vec<ChatEntry> = storage::list_chats(offset)
        .into_iter()
        .map(|(id, title)| crate::chat_list::entry(id, title))
        .collect();
    ui.set_chats(ModelRc::new(Rc::new(VecModel::from(entries))));
    update_chat_title(ui);
}

fn update_chat_title(ui: &AppWindow) {
    let id = ui.get_current_chat_id();
    let title = if id.is_empty() {
        String::new()
    } else {
        storage::chat_title(&id).unwrap_or_default()
    };
    ui.set_current_chat_title(title.into());
    activity::show(ui);
}

fn open_chat(ui: &AppWindow, id: &str) {
    if ui.get_busy() {
        return;
    }
    match storage::load_chat(id) {
        Ok(chat) => {
            let messages = chat
                .messages
                .iter()
                .map(|m| chat_message(m.role == "user", &m.content, &m.attachments, &m.status, &m.info))
                .collect();
            set_messages(ui, messages);
            folders::set(ui, &sandbox::from_json(&chat.folders));
            groups_ui::set_current(ui, &chat.group);
            coding_ui::set(ui, coding::Mode::from_json(&chat.mode));
            ui.set_think_mode(storage::chat_think(&chat.id));
            ui.set_current_chat_id(chat.id.into());
            crate::auto_sync::refresh(ui);
            crate::work_ui::refresh(ui);
            mark_checkpoints(ui);
            crate::preview::chat_changed(ui);
            update_chat_title(ui);
            if ui.get_models().iter().any(|m| m.as_str() == chat.model) {
                ui.set_selected_model(chat.model.into());
            }
            scroll_later(ui);
        }
        Err(e) => ui.set_status(trf("Could not load chat: {}", &[&e]).into()),
    }
}

fn rename_chat(ui: &AppWindow, id: &str, title: &str) {
    let title = title.trim();
    if title.is_empty() || ui.get_busy() {
        return;
    }
    if let Err(e) = storage::rename_chat(id, title) {
        ui.set_status(trf("Could not rename chat: {}", &[&e]).into());
    }
    refresh_chat_list(ui);
}

fn delete_chat(ui: &AppWindow, id: &str) {
    if ui.get_busy() {
        return;
    }
    if let Ok(chat) = storage::load_chat(id) {
        workspace::delete_chat(&chat.group, id);
        checkpoint::delete_chat(id);
        crate::bwrap::forget_chat(id);
        for folder in sandbox::from_json(&chat.folders) {
            if workspace::is_inside(&folder.path) && folder.path.contains(id) {
                let _ = std::fs::remove_dir_all(&folder.path);
            }
        }
    }
    if let Err(e) = storage::delete_chat(id) {
        ui.set_status(trf("Could not delete chat: {}", &[&e]).into());
    }
    if ui.get_current_chat_id().as_str() == id {
        set_messages(ui, Vec::new());
        ui.set_current_chat_id("".into());
        folders::set(ui, &[]);
        groups_ui::set_current(ui, "");
        coding_ui::reset(ui);
    }
    refresh_chat_list(ui);
}

pub fn save_current_chat(ui: &AppWindow) {
    let messages: Vec<StoredMessage> = ui
        .get_messages()
        .iter()
        .map(|m| StoredMessage {
            role: if m.is_user { "user" } else { "assistant" }.to_string(),
            content: m.text.to_string(),
            attachments: m.attachments_json.to_string(),
            status: m.status.to_string(),
            info: m.info.to_string(),
        })
        .collect();
    let id = ui.get_current_chat_id().to_string();
    let Some(first) = messages.iter().find(|m| m.role == "user") else {
        if !id.is_empty() {
            let _ = storage::delete_chat(&id);
            ui.set_current_chat_id("".into());
        }
        refresh_chat_list(ui);
        return;
    };

    let first_text = if first.content.trim().is_empty() {
        attachments::names(&attachments::from_json(&first.attachments))
    } else {
        first.content.clone()
    };
    let title = storage::chat_title(&id).unwrap_or_else(|| default_title(&first_text));

    let chat = ChatFile {
        id,
        title,
        model: ui.get_selected_model().to_string(),
        messages,
        folders: sandbox::to_json(&folders::current(ui)),
        group: ui.get_current_group().to_string(),
        mode: coding_ui::current().to_json(),
    };
    if let Err(e) = storage::save_chat(&chat) {
        ui.set_status(trf("Could not save chat: {}", &[&e]).into());
    }
    let _ = storage::set_chat_think(&chat.id, ui.get_think_mode());
    refresh_chat_list(ui);
}

pub fn default_title(text: &str) -> String {
    let mut title: String = text.chars().take(40).collect();
    if text.chars().count() > 40 {
        title.push('…');
    }
    title
}

fn load_models(weak: slint::Weak<AppWindow>) {
    thread::spawn(move || {
        let result = ollama::list_models();
        let mut extra = crate::cloud_ui::active_names();
        for server in crate::servers_ui::active_servers() {
            if let Ok(models) = crate::openai_compat::list_models(&server) {
                extra.extend(models.iter().map(|id| crate::openai_compat::model_name(&server, id)));
            }
        }
        let _ = weak.upgrade_in_event_loop(move |ui| match result {
            Ok(installed) if installed.is_empty() && extra.is_empty() => {
                ui.set_ollama_running(true);
                models_page::update(&ui, &installed);
                ui.set_models(ModelRc::default());
                ui.set_status(tr("No models installed. Install one on the Models page.").into());
            }
            Ok(installed) => {
                ui.set_ollama_running(true);
                models_page::update(&ui, &installed);
                let current = ui.get_selected_model();
                let list: Vec<SharedString> = installed
                    .iter()
                    .map(|m| SharedString::from(m.name.as_str()))
                    .chain(extra.iter().map(|name| SharedString::from(name.as_str())))
                    .collect();
                let saved = SharedString::from(settings::load().last_model);
                let wanted = if list.contains(&current) {
                    current
                } else if list.contains(&saved) {
                    saved
                } else {
                    list[0].clone()
                };
                ui.set_models(ModelRc::new(Rc::new(VecModel::from(list))));
                let weak = ui.as_weak();
                Timer::single_shot(Duration::from_millis(0), move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_selected_model(wanted);
                    }
                });
                ui.set_status("".into());
            }
            Err(_) => {
                ui.set_ollama_running(false);
                models_page::update(&ui, &[]);
                let cloud: Vec<SharedString> = extra.iter().map(|name| SharedString::from(name.as_str())).collect();
                if let Some(first) = cloud.first().cloned() {
                    let current = ui.get_selected_model();
                    let wanted = if cloud.contains(&current) { current } else { first };
                    ui.set_models(ModelRc::new(Rc::new(VecModel::from(cloud))));
                    ui.set_selected_model(wanted);
                } else {
                    ui.set_models(ModelRc::default());
                }
                ui.set_status(tr("Ollama is not running. Start it at the bottom left.").into());
            }
        });
    });
}

pub(crate) fn system_message() -> Message {
    let current = settings::load();
    let profile = crate::profiles_ui::active().unwrap_or_default();
    let mut text = String::from("You are a helpful assistant running locally on the user's computer.");
    if !profile.name.trim().is_empty() {
        text.push_str(&format!("\nThe user's name is {}.", profile.name.trim()));
    }
    if let Some(language) = crate::profile::answer_language_name(&current.answer_language) {
        text.push_str(&format!(
            "\nAlways answer in {language}, even if the user writes in another language."
        ));
    }
    if !profile.about.trim().is_empty() {
        text.push_str(&format!("\nAbout the user: {}", profile.about.trim()));
    }
    if !current.system_prompt.trim().is_empty() {
        text.push_str(&format!("\n\n{}", current.system_prompt.trim()));
    }
    let group = groups::load(&groups_ui::current_id());
    let use_global = group.as_ref().is_none_or(|group| group.use_global_start_args);
    let (extra, removed) = group
        .map(|group| (group.extra_start_args, group.removed_start_args))
        .unwrap_or_default();
    let args = start_args::for_group(&start_args::load(), &extra, &removed, use_global);
    if let Some(note) = start_args::system_note(&args) {
        text.push_str("\n\n");
        text.push_str(&note);
    }
    Message {
        role: "system".to_string(),
        content: text,
        ..Default::default()
    }
}

pub(crate) fn history(messages: impl Iterator<Item = ChatMessage>) -> Vec<Message> {
    let mut messages: Vec<ChatMessage> = messages.collect();
    let marker = messages.iter().rposition(|m| m.status == crate::compact::STATUS);
    let summary = marker.map(|index| crate::compact::summary_of(&messages[index].text));
    if let Some(index) = marker {
        messages.drain(..=index);
    }
    let newest_with_images = messages
        .iter()
        .rposition(|m| {
            m.is_user
                && !attachments::images(&attachments::from_json(&m.attachments_json)).is_empty()
        })
        .filter(|index| {
            messages
                .get(index + 1)
                .is_none_or(|reply| reply.status != "error")
        });

    let mut system = system_message();
    if let Some(summary) = summary {
        system.content.push_str(&crate::compact::system_addition(&summary));
    }
    let mut list = vec![system];
    for (index, m) in messages.iter().enumerate() {
        if !m.is_user && m.status == "error" {
            continue;
        }
        let attached = attachments::from_json(&m.attachments_json);
        let text = if m.is_user {
            m.text.to_string()
        } else {
            context_saver::strip_thinking(&markdown::without_tool_blocks(&m.text))
        };
        let mut content = attachments::prompt_text(&text, &attached);
        let (images, image_names) = if Some(index) == newest_with_images {
            (attachments::images(&attached), attachments::image_names(&attached))
        } else {
            let names = attachments::image_names(&attached);
            if !names.is_empty() {
                content.push_str(&format!(
                    "\n\n[Images shown earlier, not sent again: {}]",
                    names.join(", ")
                ));
            }
            (Vec::new(), Vec::new())
        };
        list.push(Message {
            role: if m.is_user { "user" } else { "assistant" }.to_string(),
            content,
            images,
            image_names,
            ..Default::default()
        });
    }
    list
}

fn send_message(weak: slint::Weak<AppWindow>, text: SharedString, cancel: Arc<AtomicBool>) {
    let Some(ui) = weak.upgrade() else { return };
    let text = text.trim().to_string();
    let model = ui.get_selected_model().to_string();
    if (text.is_empty() && !composer::has_pending()) || model.is_empty() || ui.get_busy() {
        return;
    }
    let pending = composer::take_pending(&ui);
    crate::work_ui::ensure_group_test_env(&ui);
    coding_ui::before_send(&ui);

    let list = message_list();
    let folder_info = folders::label(&folders::current(&ui));
    list.push(chat_message(true, &text, &attachments::to_json(&pending), "", &folder_info));
    let request = history(list.iter());
    list.push(chat_message(false, "", "", "", ""));

    if ui.get_current_chat_id().is_empty() {
        ui.set_current_chat_id(storage::new_id().into());
    }
    let mut request = request;
    if let Some(note) = crate::auto_sync::before_send(&ui)
        && let Some(last) = request.iter_mut().rev().find(|m| m.role == "user")
    {
        last.content = format!("{note}\n\n{}", last.content);
    }
    start_stream(&ui, model, request, String::new(), cancel);
}

fn continue_reply(ui: &AppWindow, cancel: Arc<AtomicBool>) {
    let model = ui.get_selected_model().to_string();
    if ui.get_busy() || model.is_empty() {
        return;
    }
    let list = message_list();
    let count = list.row_count();
    let Some(last) = list.row_data(count.saturating_sub(1)) else {
        return;
    };
    if last.is_user {
        return;
    }
    let mut request = history(list.iter());
    request.push(Message {
        role: "user".to_string(),
        content: CONTINUE_PROMPT.to_string(),
        ..Default::default()
    });
    list.set_row_data(
        count - 1,
        chat_message(false, &last.text, "", "", ""),
    );
    start_stream(ui, model, request, last.text.to_string(), cancel);
}

fn edit_prompt(ui: &AppWindow, index: usize) {
    if ui.get_busy() {
        return;
    }
    let list = message_list();
    let Some(message) = list.row_data(index) else {
        return;
    };
    if !message.is_user {
        return;
    }
    while list.row_count() > index {
        list.remove(list.row_count() - 1);
    }
    composer::restore_pending(ui, attachments::from_json(&message.attachments_json));
    ui.set_draft(message.text.clone());
    save_current_chat(ui);
}

fn start_stream(
    ui: &AppWindow,
    model: String,
    request: Vec<Message>,
    existing: String,
    cancel: Arc<AtomicBool>,
) {
    let current = settings::load();
    let mut config = ChatConfig {
        context_length: current.context_for(&model),
        keep_alive: current.keep_alive.clone(),
        think: None,
    };
    let think_mode = ui.get_think_mode();
    let mut limit = config.context_length as u64;
    let start_context = config.context_length;

    cancel.store(false, Ordering::SeqCst);
    ui.set_busy(true);
    scroll_later(ui);

    let weak = ui.as_weak();
    let limit_hit = Arc::new(AtomicBool::new(false));
    let mut request = request;
    let shared = folders::current(ui);
    let chat_id = ui.get_current_chat_id().to_string();
    let group_id = ui.get_current_group().to_string();
    let mode = coding_ui::current();
    let external = cloud_cli::from_model(&model).is_some()
        || crate::openai_compat::from_model(&model, &current.servers).is_some();
    let auto = current.auto_switch && existing.is_empty() && !external;
    let answer_index = message_list().row_count().saturating_sub(1);
    if existing.is_empty() {
        checkpoint::clear(&chat_id, answer_index);
    }
    let checkpoint_dir = checkpoint::dir(&chat_id, answer_index);
    thread::spawn(move || {
        let mut model = model;
        let mut start_context = start_context;
        let route_text = auto_switch::route_text(
            &request.iter().filter(|m| m.role == "user").map(|m| m.content.as_str()).collect::<Vec<_>>(),
        );
        let mut task = None;
        if auto {
            let _ = weak.upgrade_in_event_loop(|ui| ui.set_mic_status(tr("Auto-Switch: choosing a model…").into()));
            let choice = auto_switch::choose(&route_text, &model, mode.is_coding());
            task = choice.as_ref().map(|choice| choice.task);
            if let Some(choice) = &choice
                && choice.model != model
            {
                model = choice.model.clone();
                config.context_length = current.context_for(&model);
                limit = config.context_length as u64;
                start_context = config.context_length;
            }
            let chosen = model.clone();
            let note = choice.map(|choice| choice.note()).unwrap_or_default();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_mic_status("".into());
                ui.set_selected_model(chosen.into());
                if !note.is_empty() {
                    ui.set_status(note.into());
                }
            });
        }
        let has_images = request.iter().any(|m| !m.images.is_empty());
        if has_images && !ollama::supports_vision(&model) {
            let _ = weak.upgrade_in_event_loop(|ui| {
                ui.set_mic_status(tr("Model has no image support – sending images as text (colors + OCR)…").into());
            });
            for message in request.iter_mut().filter(|m| !m.images.is_empty()) {
                for (data, name) in message.images.iter().zip(message.image_names.iter()) {
                    message.content.push_str("\n\n");
                    message.content.push_str(&attachments::describe_image(name, data));
                }
                message.images.clear();
            }
            let _ = weak.upgrade_in_event_loop(|ui| ui.set_mic_status("".into()));
        }
        if ollama::supports_thinking(&model) {
            let task = task.unwrap_or_else(|| auto_switch::adjust(auto_switch::guess(&route_text), mode.is_coding()));
            let off = auto_switch::think_off(think_mode, task);
            config.think = off.then_some(false);
            if think_mode == auto_switch::THINK_AUTO {
                let note = trf(
                    "Thinking auto: {} → {}",
                    &[&tr(task.label()), &if off { tr("off") } else { tr("on") }],
                );
                let _ = weak.upgrade_in_event_loop(move |ui| ui.set_status(note.into()));
            }
        }
        let server_model = crate::openai_compat::from_model(&model, &current.servers);
        let can_use_tools = current.tools_enabled
            && match &server_model {
                Some((server, _)) => server.tools,
                None => ollama::supports_tools(&model),
            };
        let group = groups::load(&group_id);
        let memory = group.as_ref().is_some_and(|group| group.memory_enabled);
        let mut shared = shared;
        let work_note = if can_use_tools && workspace::needs_work_folder(&shared) && !chat_id.is_empty() {
            prepare_work_folder(&weak, &cancel, &group_id, &chat_id, &mut shared)
        } else {
            shared
                .iter()
                .find(|folder| workspace::is_inside(&folder.path))
                .map(|folder| {
                    let dir = std::path::PathBuf::from(&folder.path);
                    workspace::test_env_note(&dir, workspace::has_copy(&dir))
                })
        };
        let docs = group.as_ref().and_then(|group| group.docs_folder.clone());
        let mut tools = can_use_tools.then(|| crate::rag_ui::with_docs_tool(tools::definitions(&shared, memory), docs.is_some()));
        let mut granted: Vec<sandbox::Folder> = Vec::new();
        if let Some(note) = work_note
            && let Some(system) = request.first_mut().filter(|m| m.role == "system")
        {
            system.content.push_str("\n\n");
            system.content.push_str(&note);
        }
        if let Some(group) = &group
            && let Some(system) = request.first_mut().filter(|m| m.role == "system")
        {
            crate::projects::refresh_group_index(group);
            system.content.push_str("\n\n");
            system.content.push_str(&groups::system_note(group));
        }
        if !shared.is_empty() {
            if can_use_tools {
                if let Some(system) = request.first_mut().filter(|m| m.role == "system") {
                    system.content.push_str("\n\n");
                    system.content.push_str(&file_tools::system_note(&shared));
                    if !mode.is_coding()
                        && let Some(first) = shared.first()
                    {
                        system.content.push_str("\n\n");
                        system.content.push_str(&coding::architecture_hint(std::path::Path::new(&first.path), first.writable));
                    }
                }
            } else {
                let note = if current.tools_enabled {
                    tr("This model can't use tools, so it can't see the shared folders.")
                } else {
                    tr("Tools are turned off in Settings, so the model can't see the shared folders.")
                };
                let _ = weak.upgrade_in_event_loop(move |ui| ui.set_status(note.into()));
            }
        }
        let project = coding_ui::project_folder(&shared);
        let coding_note = coding::system_note(
            &mode,
            project.as_ref().map(|folder| coding::Project {
                root: std::path::Path::new(&folder.path),
                tools: can_use_tools,
                writable: folder.writable,
                memory: can_use_tools && memory,
            }).as_ref(),
        );
        if let Some(note) = coding_note
            && let Some(system) = request.first_mut().filter(|m| m.role == "system")
        {
            system.content.push_str("\n\n");
            system.content.push_str(&note);
        }
        let mut stream = Stream {
            weak: weak.clone(),
            reply: existing,
            buffer: String::new(),
            last_flush: Instant::now(),
        };
        let mut generated: u64 = 0;
        let mut prompt_estimate = ollama::estimate_tokens(&request);
        let mut result: Result<ollama::ChatOutcome, String>;

        let mut step = 0;
        let mut reads = context_saver::Reads::default();
        let mut step_starts: Vec<usize> = Vec::new();
        let mut steps_hit = false;
        let mut repeat_hit = false;
        let mut pause: Option<(String, u64)> = None;
        let mut recent_steps: Vec<String> = Vec::new();
        let mut freed_others = false;
        let mut crash_retried = false;
        let mut nudges = 0;
        let short_model = model.rsplit('/').next().unwrap_or(&model).to_string();
        let thinking_label = trf("Thinking ({})", &[&short_model]);
        let writing_label = trf("Writing the answer ({})", &[&short_model]);
        let answer_id = activity::start(&weak, &chat_id, "🤖", &thinking_label);
        let mut writing = false;
        let max_steps = if mode.is_coding() { tools::MAX_CODING_STEPS } else { tools::MAX_STEPS };
        let mut files_changed = false;
        let mut check_rounds = 0;
        let ask_weak = weak.clone();
        let ask_cancel = cancel.clone();
        let ask = move |command: &str, directory: &std::path::Path, read_only: bool| {
            approval::ask(&ask_weak, &ask_cancel, command, directory, read_only)
        };
        let web_weak = weak.clone();
        let web_cancel = cancel.clone();
        let ask_web = move |title: &str, text: &str, note: &str| {
            approval::show(&web_weak, &web_cancel, title, text, note, "")
        };
        loop {
            let step_start = stream.reply.len();
            let thread_limit = limit_hit.clone();
            let thread_cancel = cancel.clone();
            let mut in_think = false;
            let on_chunk = |chunk: &str, thinking: bool| {
                if thinking && !in_think {
                    in_think = true;
                    stream.push(&format!("\n{}\n", markdown::THINK_FENCE));
                } else if !thinking && in_think {
                    in_think = false;
                    stream.push("\n```\n");
                }
                if !thinking && !writing && !chunk.trim().is_empty() {
                    writing = true;
                    activity::rename(&weak, &chat_id, answer_id, &writing_label);
                }
                stream.push(chunk);
                generated += 1;
                let used = prompt_estimate + generated;
                let near = used * 100 >= limit * 95;
                let hard = used * 100 >= limit * 99;
                let good_break = chunk.contains('\n')
                    || chunk.trim_end().ends_with(['.', '!', '?', ':'])
                        && chunk.ends_with([' ', '\n', '.', '!', '?', ':']);
                if hard || (near && good_break && !markdown::inside_code(&stream.reply)) {
                    thread_limit.store(true, Ordering::SeqCst);
                    thread_cancel.store(true, Ordering::SeqCst);
                }
            };
            result = if let Some(provider) = cloud_cli::from_model(&model) {
                cloud_cli::chat(provider, &request, &shared, &cancel, on_chunk)
            } else if let Some((server, id)) = &server_model {
                crate::openai_compat::chat_stream(server, id, &request, tools.as_ref(), &cancel, on_chunk)
            } else {
                ollama::chat_stream(&model, &request, &config, tools.as_ref(), &cancel, on_chunk)
            };

            if in_think {
                stream.push("\n```\n");
            }
            let out_of_memory = matches!(&result, Err(e) if ollama::is_out_of_memory(e))
                && stream.reply.len() == step_start
                && !cancel.load(Ordering::SeqCst);
            if out_of_memory && !freed_others {
                freed_others = true;
                let count = ollama::unload_others(&model);
                if count > 0 {
                    let note = trf("Not enough VRAM – unloaded {} other model(s), trying again…", &[&count]);
                    let _ = weak.upgrade_in_event_loop(move |ui| ui.set_status(note.into()));
                    continue;
                }
            }
            let crashed = matches!(&result, Err(e) if ollama::is_runner_crash(e))
                && stream.reply.len() == step_start
                && !cancel.load(Ordering::SeqCst);
            if crashed && !crash_retried {
                crash_retried = true;
                config.context_length = (config.context_length / 2).max(MIN_CONTEXT);
                limit = config.context_length as u64;
                let note = crate::i18n::tr("The model runner crashed – trying once more with a smaller context…");
                let _ = weak.upgrade_in_event_loop(move |ui| ui.set_status(note.into()));
                thread::sleep(Duration::from_secs(2));
                continue;
            }
            if out_of_memory && config.context_length > MIN_CONTEXT {
                let old = config.context_length;
                config.context_length = (old / 2).max(MIN_CONTEXT);
                limit = config.context_length as u64;
                let fits = config.context_length;
                settings::update(|s| {
                    s.model_context.insert(model.clone(), fits);
                });
                let note = trf(
                    "Not enough VRAM for {} context – trying {}…",
                    &[&short_count(old as u64), &short_count(fits as u64)],
                );
                let _ = weak.upgrade_in_event_loop(move |ui| ui.set_status(note.into()));
                continue;
            }

            let calls = match &result {
                Ok(outcome) if !outcome.tool_calls.is_empty() => outcome.tool_calls.clone(),
                Ok(_) if tools.is_some()
                    && !shared.is_empty()
                    && nudges < nudge::MAX
                    && !cancel.load(Ordering::SeqCst)
                    && nudge::stalled(&markdown::without_tool_blocks(&stream.reply[step_start..])) =>
                {
                    nudges += 1;
                    request.push(Message {
                        role: "assistant".to_string(),
                        content: markdown::without_tool_blocks(&stream.reply[step_start..]),
                        ..Default::default()
                    });
                    request.push(Message {
                        role: "user".to_string(),
                        content: nudge::MESSAGE.to_string(),
                        ..Default::default()
                    });
                    stream.push("\n\n");
                    continue;
                }
                Ok(_) if mode.is_coding()
                    && files_changed
                    && check_rounds < auto_check::MAX_ROUNDS
                    && !cancel.load(Ordering::SeqCst) =>
                {
                    files_changed = false;
                    let Some(feedback) = run_auto_check(
                        &weak,
                        &chat_id,
                        &group_id,
                        &shared,
                        &current.safe_commands,
                        &cancel,
                        &ask,
                        &ask_web,
                        &mut stream,
                        &mut check_rounds,
                    ) else {
                        break;
                    };
                    request.push(Message {
                        role: "assistant".to_string(),
                        content: markdown::without_tool_blocks(&stream.reply[step_start..]),
                        ..Default::default()
                    });
                    request.push(Message {
                        role: "user".to_string(),
                        content: feedback,
                        ..Default::default()
                    });
                    stream.push("\n\n");
                    continue;
                }
                _ => break,
            };
            if cancel.load(Ordering::SeqCst) {
                break;
            }
            step += 1;
            if step > max_steps {
                steps_hit = true;
                break;
            }

            let step_text = markdown::without_tool_blocks(&stream.reply[step_start..]);
            recent_steps.push(normalized_step(&step_text));
            if repeats(&recent_steps) {
                repeat_hit = true;
                break;
            }
            step_starts.push(request.len());
            request.push(Message {
                role: "assistant".to_string(),
                content: step_text,
                tool_calls: calls.clone(),
                ..Default::default()
            });
            let context = tools::Context {
                folders: &shared,
                chat_id: &chat_id,
                group: &group_id,
                safe: &current.safe_commands,
                cancel: Some(cancel.clone()),
                ask: &ask,
                ask_web: &ask_web,
                checkpoint: Some(checkpoint_dir.as_path()),
            };
            for call in &calls {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let (icon, label) = tools::activity(call);
                let tool_id = activity::start(&weak, &chat_id, icon, &label);
                let ticker = (call.function.name == crate::commands::NAME).then(|| {
                    start_ticker(weak.clone(), chat_id.clone(), tool_id, label.clone())
                });
                let output = if call.function.name == "pause_and_wait" {
                    let reason = call.function.arguments["reason"].as_str().unwrap_or("something").trim().to_string();
                    let minutes = call.function.arguments["minutes"]
                        .as_u64()
                        .or_else(|| call.function.arguments["minutes"].as_str().and_then(|m| m.trim().parse().ok()))
                        .unwrap_or(5)
                        .clamp(1, 30);
                    let note = format!("Paused for about {minutes} minutes. Stop now; the answer continues later.");
                    pause = Some((reason, minutes));
                    note
                } else if call.function.name == crate::rag_ui::TOOL {
                    crate::rag_ui::run_tool(docs.as_deref(), &call.function.arguments)
                } else if call.function.name == "request_project_access" {
                    grant_project(&weak, &cancel, &call.function.arguments, &shared, &mut granted)
                } else if call.function.name == crate::question::NAME {
                    crate::question::ask(&weak, &cancel, &call.function.arguments)
                } else {
                    match reads.already_read(call, &shared, &request) {
                        Some(note) => note,
                        None => tools::run(call, &context),
                    }
                };
                if let Some(ticker) = ticker {
                    ticker.store(true, Ordering::SeqCst);
                }
                if auto_check::changes_files(&call.function.name, &output) {
                    files_changed = true;
                }
                let failed = output.starts_with("Error")
                    || output.starts_with("The user did not allow")
                    || output.contains("[exit code");
                let first_line = output
                    .lines()
                    .find(|line| !line.trim().is_empty() && !line.starts_with("[Content from the web"))
                    .unwrap_or_default();
                activity::finish(
                    &weak,
                    &chat_id,
                    tool_id,
                    if failed { activity::FAILED } else { activity::DONE },
                    first_line,
                );
                if call.function.name == "remember" {
                    let _ = weak.upgrade_in_event_loop(|ui| crate::start_args_page::refresh(&ui));
                }
                stream.push(&markdown::tool_block(&tools::describe_call(call), &output));
                stream.flush();
                let fresh_read = !output.starts_with("Error") && !output.starts_with("Already read");
                request.push(Message {
                    role: "tool".to_string(),
                    content: output,
                    tool_name: Some(call.function.name.clone()),
                    ..Default::default()
                });
                if fresh_read {
                    reads.remember(call, &shared, request.len() - 1);
                }
            }
            if pause.is_some() {
                break;
            }
            if !granted.is_empty() {
                shared.append(&mut granted);
                tools = can_use_tools.then(|| crate::rag_ui::with_docs_tool(tools::definitions(&shared, memory), docs.is_some()));
                let list = shared.clone();
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    let mut current = folders::current(&ui);
                    for folder in list {
                        if !current.iter().any(|known| known.path == folder.path) && !workspace::is_inside(&folder.path) {
                            current.push(folder);
                        }
                    }
                    folders::changed(&ui, &current);
                });
            }
            prompt_estimate = ollama::estimate_tokens(&request);
            if prompt_estimate * 100 > limit * 55 && step_starts.len() >= 3 {
                let keep_from = step_starts[step_starts.len() - 2];
                if context_saver::shorten_old(&mut request, keep_from) > 0 {
                    prompt_estimate = ollama::estimate_tokens(&request);
                }
            }
            generated = 0;
            writing = false;
            activity::rename(&weak, &chat_id, answer_id, &thinking_label);
        }
        stream.flush();

        let status = if result.is_err() {
            "error"
        } else if limit_hit.load(Ordering::SeqCst) {
            "limit"
        } else if steps_hit {
            "steps"
        } else if repeat_hit {
            "repeat"
        } else if pause.is_some() {
            "pause"
        } else if cancel.load(Ordering::SeqCst) {
            "stopped"
        } else {
            ""
        };
        let reduced = config.context_length < start_context;
        let reduced_to = short_count(config.context_length as u64);
        let info = match &result {
            Ok(ollama::ChatOutcome {
                stats:
                    Some(ChatStats {
                        prompt_tokens,
                        output_tokens,
                    }),
                ..
            }) => context_info(prompt_tokens + output_tokens, limit, false),
            _ => context_info(prompt_estimate + generated, limit, true),
        };
        let auto_continue = current.auto_continue;
        let pause_minutes = pause.as_ref().map(|(_, minutes)| *minutes).unwrap_or(0);
        let info = match &pause {
            Some((reason, minutes)) => {
                let when = (chrono::Local::now() + chrono::Duration::minutes(*minutes as i64)).format("%H:%M");
                if auto_continue {
                    trf("⏸ {} · continues automatically at {} · {}", &[&reason, &when, &info])
                } else {
                    trf("⏸ {} · about {} min · {}", &[&reason, &minutes, &info])
                }
            }
            None => info,
        };
        let (state, detail) = match (&result, status) {
            (Err(e), _) => (activity::FAILED, e.lines().next().unwrap_or_default().to_string()),
            (_, "stopped") => (activity::STOPPED, tr("Stopped")),
            _ => (activity::DONE, info.clone()),
        };
        activity::finish(&weak, &chat_id, answer_id, state, &detail);
        let _ = weak.upgrade_in_event_loop(move |ui| {
            let failed = result.is_err();
            if let Err(e) = result {
                append_to_last(&ui, &trf("[Error: {}]", &[&e]));
                if ollama::is_out_of_memory(&e) {
                    append_to_last(
                        &ui,
                        &tr("\n\nThe model does not fit into your VRAM/RAM, even with a small context. Try a smaller model or a smaller quantization (e.g. Q4 instead of Q8), or close other programs that use the graphics card."),
                    );
                }
                if ollama::is_runner_crash(&e) {
                    append_to_last(&ui, &format!("\n\n{}", crate::i18n::tr(ollama::CRASH_HELP)));
                }
            } else if reduced {
                ui.set_status(
                    trf("Context for this model reduced to {} so it fits into VRAM (remembered).", &[&reduced_to])
                        .into(),
                );
            }
            finish_last(status, &info);
            update_context_bar(&ui);
            ui.set_busy(false);
            crate::work_ui::refresh(&ui);
            mark_checkpoints(&ui);
            crate::preview::refresh(&ui);
            coding_ui::refresh(&ui);
            save_current_chat(&ui);
            crate::chat_list::auto_title(&ui);
            if status == "pause" && auto_continue {
                schedule_continue(&ui, pause_minutes);
            } else if !failed && status != "stopped" && status != "pause" {
                crate::compact::after_answer(&ui);
            }
            scroll_later(&ui);
            ui.window().request_redraw();
        });
    });
}

struct Stream {
    weak: slint::Weak<AppWindow>,
    reply: String,
    buffer: String,
    last_flush: Instant,
}

impl Stream {
    fn push(&mut self, text: &str) {
        self.reply.push_str(text);
        self.buffer.push_str(text);
        if self.last_flush.elapsed() >= Duration::from_millis(50) {
            self.flush();
        }
    }

    fn flush(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.buffer);
        self.last_flush = Instant::now();
        let _ = self
            .weak
            .upgrade_in_event_loop(move |ui| append_to_last(&ui, &text));
    }
}

pub(crate) fn context_info(tokens: u64, limit: u64, estimated: bool) -> String {
    let prefix = if estimated { "~" } else { "" };
    format!(
        "{prefix}{} / {} tokens context",
        short_count(tokens),
        short_count(limit)
    )
}

fn prepare_work_folder(
    weak: &slint::Weak<AppWindow>,
    cancel: &Arc<AtomicBool>,
    group_id: &str,
    chat_id: &str,
    shared: &mut Vec<sandbox::Folder>,
) -> Option<String> {
    let dir = workspace::chat_dir(group_id, chat_id);
    let mut copied = false;
    if !dir.exists() {
        let answer = approval::show(
            weak,
            cancel,
            &tr("Work folder for this chat?"),
            &trf(
                "The shared folder is read-only. May the model get its own work folder with a copy of the project to test changes?\n{}",
                &[&dir.display()],
            ),
            &tr("The folder is deleted together with its group. Change the base folder in Settings."),
            "",
        );
        if matches!(answer, crate::commands::Approval::No) {
            return None;
        }
        let project = shared.first().map(|folder| std::path::PathBuf::from(&folder.path));
        let result = match &project {
            Some(project) => workspace::copy_project(project, &dir).map(|_| true),
            None => std::fs::create_dir_all(&dir).map(|_| false).map_err(|e| e.to_string()),
        };
        match result {
            Ok(done) => copied = done,
            Err(e) => {
                let note = trf("Could not create the work folder: {}", &[&e]);
                let _ = weak.upgrade_in_event_loop(move |ui| ui.set_status(note.into()));
                if !dir.exists() {
                    return None;
                }
            }
        }
    }
    shared.push(sandbox::Folder {
        path: dir.to_string_lossy().to_string(),
        writable: true,
    });
    Some(workspace::system_note(&dir, copied))
}

fn grant_project(
    weak: &slint::Weak<AppWindow>,
    cancel: &Arc<AtomicBool>,
    arguments: &serde_json::Value,
    shared: &[sandbox::Folder],
    granted: &mut Vec<sandbox::Folder>,
) -> String {
    let name = arguments["project"].as_str().unwrap_or_default();
    let reason = arguments["reason"].as_str().unwrap_or("no reason given").trim();
    let Some(project) = crate::projects::find(name) else {
        return format!("Error: unknown project '{name}' – call list_projects first");
    };
    let path = project.folder.to_string_lossy().to_string();
    if shared.iter().chain(granted.iter()).any(|folder| folder.path == path) {
        return format!("You already have access to {path}");
    }
    if !project.folder.is_dir() {
        return format!("Error: the folder {path} does not exist");
    }
    let question = trf(
        "The model asks for access to the project \"{}\" ({}).\nReason: {}",
        &[&project.name, &path, &reason],
    );
    let ask = serde_json::json!({ "question": question, "options": ["Read only", "Read + write", "No"] });
    let answer = crate::question::ask(weak, cancel, &ask);
    let writable = answer.contains("Read + write");
    if writable || answer.contains("Read only") {
        granted.push(sandbox::Folder { path: path.clone(), writable });
        format!(
            "Access granted ({}). Use absolute paths starting with {path}. Read its ARCHITEKTUR.md first if it has one.",
            if writable { "read + write" } else { "read only" }
        )
    } else {
        "The user did not give access. Do not try again; continue without it or ask what to do.".to_string()
    }
}

#[allow(clippy::too_many_arguments)]
fn run_auto_check(
    weak: &slint::Weak<AppWindow>,
    chat_id: &str,
    group_id: &str,
    shared: &[sandbox::Folder],
    safe: &[String],
    cancel: &Arc<AtomicBool>,
    ask: &dyn Fn(&str, &std::path::Path, bool) -> crate::commands::Approval,
    ask_web: &dyn Fn(&str, &str, &str) -> crate::commands::Approval,
    stream: &mut Stream,
    rounds: &mut usize,
) -> Option<String> {
    let root = shared.first()?;
    let command = auto_check::command(std::path::Path::new(&root.path))?;
    *rounds += 1;
    let label = trf("Auto check: {}", &[&command]);
    let tool_id = activity::start(weak, chat_id, "🔍", &label);
    let ticker = start_ticker(weak.clone(), chat_id.to_string(), tool_id, label);
    let context = tools::Context {
        folders: shared,
        chat_id,
        group: group_id,
        safe,
        cancel: Some(cancel.clone()),
        ask,
        ask_web,
        checkpoint: None,
    };
    let output = crate::commands::run(&serde_json::json!({ "command": command }), &context);
    ticker.store(true, Ordering::SeqCst);
    let outcome = auto_check::judge(&output);
    let first_line = output.lines().find(|line| !line.trim().is_empty()).unwrap_or_default();
    activity::finish(
        weak,
        chat_id,
        tool_id,
        if outcome == auto_check::Outcome::Problems { activity::FAILED } else { activity::DONE },
        first_line,
    );
    stream.push(&markdown::tool_block(&format!("auto_check({command})"), &output));
    stream.flush();
    match outcome {
        auto_check::Outcome::Problems => Some(auto_check::feedback(command, &output, *rounds)),
        auto_check::Outcome::Clean => None,
        auto_check::Outcome::Skipped => {
            *rounds = auto_check::MAX_ROUNDS;
            None
        }
    }
}

fn normalized_step(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphabetic() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn repeats(steps: &[String]) -> bool {
    match steps {
        [.., a, b, c] => !c.is_empty() && a == b && b == c,
        _ => false,
    }
}

fn start_ticker(weak: slint::Weak<AppWindow>, chat: String, id: i32, label: String) -> Arc<AtomicBool> {
    let done = Arc::new(AtomicBool::new(false));
    let stop = done.clone();
    thread::spawn(move || {
        let started = Instant::now();
        while !stop.load(Ordering::SeqCst) {
            thread::sleep(std::time::Duration::from_secs(1));
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let secs = started.elapsed().as_secs();
            activity::rename(&weak, &chat, id, &format!("⏳ {label} · {}:{:02}", secs / 60, secs % 60));
        }
    });
    done
}

fn schedule_continue(ui: &AppWindow, minutes: u64) {
    let weak = ui.as_weak();
    let chat = ui.get_current_chat_id().to_string();
    Timer::single_shot(std::time::Duration::from_secs(minutes.max(1) * 60), move || {
        let Some(ui) = weak.upgrade() else { return };
        let list = message_list();
        let paused = list
            .row_data(list.row_count().saturating_sub(1))
            .is_some_and(|message| message.status == "pause");
        if ui.get_current_chat_id() == chat && !ui.get_busy() && paused {
            ui.invoke_continue_reply();
        }
    });
}

pub fn update_context_bar(ui: &AppWindow) {
    let messages = message_list();
    let info = (0..messages.row_count())
        .rev()
        .filter_map(|index| messages.row_data(index))
        .find(|message| !message.is_user && !message.info.is_empty())
        .map(|message| message.info.to_string())
        .unwrap_or_default();
    match parse_context_info(&info) {
        Some((used, limit)) if limit > 0.0 => {
            ui.set_context_used((used / limit).min(1.0) as f32);
            ui.set_context_label(format!("{} / {}", short_count(used as u64), short_count(limit as u64)).into());
        }
        _ => {
            ui.set_context_used(0.0);
            ui.set_context_label("".into());
        }
    }
}

fn parse_context_info(info: &str) -> Option<(f64, f64)> {
    let text = info.trim().trim_start_matches('~');
    let (used, rest) = text.split_once(" / ")?;
    let limit = rest.split_whitespace().next()?;
    let number = |value: &str| match value.strip_suffix('k') {
        Some(thousands) => thousands.parse::<f64>().ok().map(|v| v * 1000.0),
        None => value.parse::<f64>().ok(),
    };
    Some((number(used)?, number(limit)?))
}

fn short_count(count: u64) -> String {
    if count >= 1000 {
        format!("{:.1}k", count as f64 / 1000.0)
    } else {
        count.to_string()
    }
}

fn append_to_last(ui: &AppWindow, chunk: &str) {
    let messages = message_list();
    let count = messages.row_count();
    if count == 0 {
        return;
    }
    let Some(mut last) = messages.row_data(count - 1) else {
        return;
    };
    let text = format!("{}{}", last.text, chunk);
    let fresh = segments(&text);
    match last.segments.as_any().downcast_ref::<VecModel<MessageSegment>>() {
        Some(existing) => {
            for (index, segment) in fresh.into_iter().enumerate() {
                if index < existing.row_count() {
                    if existing.row_data(index).as_ref() != Some(&segment) {
                        existing.set_row_data(index, segment);
                    }
                } else {
                    existing.push(segment);
                }
            }
        }
        None => last.segments = ModelRc::new(Rc::new(VecModel::from(fresh))),
    }
    last.text = text.into();
    messages.set_row_data(count - 1, last);
    scroll_later(ui);
}

fn scroll_later(ui: &AppWindow) {
    let weak = ui.as_weak();
    Timer::single_shot(Duration::from_millis(16), move || {
        if let Some(ui) = weak.upgrade() {
            ui.invoke_scroll_to_bottom();
            let weak = ui.as_weak();
            Timer::single_shot(Duration::from_millis(60), move || {
                if let Some(ui) = weak.upgrade() {
                    ui.invoke_scroll_to_bottom();
                }
            });
        }
    });
}

fn finish_last(status: &str, info: &str) {
    let messages = message_list();
    let count = messages.row_count();
    if count == 0 {
        return;
    }
    let Some(last) = messages.row_data(count - 1) else {
        return;
    };
    if last.is_user {
        return;
    }
    if last.text.is_empty() && status.is_empty() {
        messages.remove(count - 1);
        return;
    }
    messages.set_row_data(
        count - 1,
        chat_message(false, &last.text, "", status, info),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_repeated_steps() {
        let steps: Vec<String> = ["Der Build läuft noch. Ich warte 1:", "Der Build läuft noch. Ich warte 2:", "der build läuft noch ich warte:"]
            .iter()
            .map(|text| normalized_step(text))
            .collect();
        assert!(repeats(&steps));
        assert!(!repeats(&steps[..2]));
        assert!(!repeats(&["a".to_string(), "b".to_string(), "b".to_string()]));
        assert!(!repeats(&[String::new(), String::new(), String::new()]));
    }

    #[test]
    fn info_text() {
        assert_eq!(context_info(12_345, 65_536, false), "12.3k / 65.5k tokens context");
        assert_eq!(context_info(900, 4096, true), "~900 / 4.1k tokens context");
    }
}
