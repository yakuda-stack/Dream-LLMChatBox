use crate::AppWindow;
use serde_json::Value;
use slint::{ModelRc, SharedString, VecModel};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const NAME: &str = "ask_user";
pub const MAX_OPTIONS: usize = 4;

static PENDING: Mutex<Option<Sender<String>>> = Mutex::new(None);

pub fn setup(ui: &AppWindow) {
    ui.on_question_answer(|answer| {
        if let Some(sender) = PENDING.lock().ok().and_then(|mut pending| pending.take()) {
            let _ = sender.send(answer.trim().to_string());
        }
    });
}

pub fn options(arguments: &Value) -> Vec<String> {
    let list: Vec<String> = match &arguments["options"] {
        Value::Array(items) => items.iter().filter_map(|item| item.as_str().map(str::to_string)).collect(),
        Value::String(text) => text.split(['\n', ';', '|']).map(str::to_string).collect(),
        _ => Vec::new(),
    };
    list.into_iter()
        .map(|option| option.trim().to_string())
        .filter(|option| !option.is_empty())
        .take(MAX_OPTIONS)
        .collect()
}

pub fn ask(weak: &slint::Weak<AppWindow>, cancel: &Arc<AtomicBool>, arguments: &Value) -> String {
    let question = arguments["question"].as_str().unwrap_or_default().trim().to_string();
    if question.is_empty() {
        return "Error: question is empty".to_string();
    }
    let choices = options(arguments);
    let (sender, receiver) = mpsc::channel();
    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some(sender);
    }
    let shown = choices.clone();
    let _ = weak.upgrade_in_event_loop(move |ui| {
        let items: Vec<SharedString> = shown.into_iter().map(SharedString::from).collect();
        ui.set_question_text(question.into());
        ui.set_question_options(ModelRc::new(VecModel::from(items)));
        ui.set_question_open(true);
    });
    let answer = loop {
        match receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(answer) => break Some(answer),
            Err(RecvTimeoutError::Timeout) if cancel.load(Ordering::SeqCst) => break None,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break None,
        }
    };
    if let Ok(mut pending) = PENDING.lock() {
        *pending = None;
    }
    let _ = weak.upgrade_in_event_loop(|ui| ui.set_question_open(false));
    match answer {
        Some(answer) if choices.contains(&answer) => format!("The user chose: {answer}"),
        Some(answer) if !answer.is_empty() => format!("The user answered in own words: {answer}"),
        _ => "The user did not answer. Do not guess – stop here and say what you still need.".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn options_are_cleaned_and_limited() {
        let list = options(&json!({"options": ["A", " ", "B", "C", "D", "E"]}));
        assert_eq!(list, vec!["A", "B", "C", "D"]);
        assert_eq!(options(&json!({"options": "Ja; Nein"})), vec!["Ja", "Nein"]);
        assert!(options(&json!({})).is_empty());
    }
}
