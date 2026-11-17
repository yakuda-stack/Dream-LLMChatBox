use crate::i18n::{tr, trf};
use crate::chat;
use crate::cloud_cli;
use crate::context_saver;
use crate::markdown;
use crate::ollama::{self, ChatConfig, Message};
use crate::settings;
use crate::{AppWindow, ChatMessage};
use slint::{ComponentHandle, Model};
use std::sync::atomic::AtomicBool;
use std::thread;

pub const STATUS: &str = "compact";
pub const LANGUAGE: &str = "dream-compact";
const KEEP: usize = 4;
const MAX_MESSAGE_CHARS: usize = 4000;
const PROMPT: &str = "You compress a chat so it can go on with less context. Write a compact summary in the language of the conversation. Keep the user's goals and wishes, decisions, facts, names, file paths, numbers, code that is still needed, and open tasks and questions. Leave out small talk and repetition. Use short bullet points, at most about 400 words. Answer only with the summary.";

pub fn block(summary: &str) -> String {
    format!("```{LANGUAGE}\n{}\n```", summary.trim())
}

pub fn summary_of(text: &str) -> String {
    text.trim()
        .trim_start_matches(&format!("```{LANGUAGE}"))
        .trim_end_matches("```")
        .trim()
        .to_string()
}

pub fn system_addition(summary: &str) -> String {
    format!("\n\nSummary of the earlier conversation (older messages were compressed to save context):\n{summary}")
}

pub struct Part {
    pub is_user: bool,
    pub text: String,
    pub status: String,
}

pub fn range(parts: &[Part]) -> Option<(usize, usize)> {
    let from = parts
        .iter()
        .rposition(|part| part.status == STATUS)
        .map_or(0, |index| index + 1);
    let newest = parts.len().checked_sub(KEEP)?;
    let to = (from..=newest).rev().find(|index| parts[*index].is_user)?;
    let real = parts[from..to].iter().filter(|part| part.status != "error").count();
    (real >= 2).then_some((from, to))
}

fn clean(part: &Part) -> String {
    let text = if part.is_user {
        part.text.clone()
    } else {
        context_saver::strip_thinking(&markdown::without_tool_blocks(&part.text))
    };
    let text = text.trim();
    if text.chars().count() > MAX_MESSAGE_CHARS {
        let short: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
        format!("{short} […]")
    } else {
        text.to_string()
    }
}

pub fn transcript(parts: &[Part], from: usize, to: usize, max_chars: usize) -> String {
    let mut lines: Vec<String> = parts[from..to]
        .iter()
        .filter(|part| part.status != "error" && part.status != STATUS)
        .map(|part| format!("{}: {}", if part.is_user { "User" } else { "Assistant" }, clean(part)))
        .collect();
    let earlier = from
        .checked_sub(1)
        .and_then(|index| parts.get(index))
        .filter(|part| part.status == STATUS)
        .map(|part| summary_of(&part.text));
    let mut cut = false;
    while lines.len() > 1 && lines.iter().map(|line| line.len() + 2).sum::<usize>() > max_chars {
        lines.remove(0);
        cut = true;
    }
    let mut text = String::new();
    if let Some(earlier) = earlier {
        text.push_str(&format!("Earlier summary:\n{earlier}\n\n"));
    }
    text.push_str("Conversation:\n");
    if cut {
        text.push_str("[…]\n\n");
    }
    text.push_str(&lines.join("\n\n"));
    text
}

pub fn replace_context(info: &str, new: &str) -> String {
    let pattern = regex::Regex::new(r"~?[\d.]+k? / [\d.]+k? tokens context").expect("valid regex");
    if pattern.is_match(info) {
        pattern.replace(info, new).to_string()
    } else if info.trim().is_empty() {
        new.to_string()
    } else {
        format!("{info} · {new}")
    }
}

fn summarize(model: &str, context: u32, keep_alive: &str, text: String) -> Result<String, String> {
    let messages = [
        Message { role: "system".to_string(), content: PROMPT.to_string(), ..Default::default() },
        Message { role: "user".to_string(), content: text, ..Default::default() },
    ];
    let cancel = AtomicBool::new(false);
    let mut out = String::new();
    let collect = |chunk: &str, thinking: bool| {
        if !thinking {
            out.push_str(chunk);
        }
    };
    let current = settings::load();
    match crate::openai_compat::from_model(model, &current.servers) {
        Some((server, id)) => crate::openai_compat::chat_stream(&server, &id, &messages, None, &cancel, collect)?,
        None => {
            let config = ChatConfig {
                context_length: context,
                keep_alive: keep_alive.to_string(),
                think: ollama::supports_thinking(model).then_some(false),
            };
            ollama::chat_stream(model, &messages, &config, None, &cancel, collect)?
        }
    };
    let summary = context_saver::strip_thinking(&out).trim().to_string();
    if summary.is_empty() {
        return Err(tr("the model returned an empty summary"));
    }
    Ok(summary)
}

fn parts(messages: &[ChatMessage]) -> Vec<Part> {
    messages
        .iter()
        .map(|m| Part { is_user: m.is_user, text: m.text.to_string(), status: m.status.to_string() })
        .collect()
}

pub fn after_answer(ui: &AppWindow) {
    let current = settings::load();
    let at = current.compact_at.clamp(30, 95) as f32 / 100.0;
    if current.auto_compact && ui.get_context_used() >= at {
        start(ui, true);
    }
}

pub fn start(ui: &AppWindow, automatic: bool) {
    if ui.get_busy() {
        return;
    }
    let list = chat::message_list();
    let messages: Vec<ChatMessage> = list.iter().collect();
    let all = parts(&messages);
    let Some((from, to)) = range(&all) else {
        if !automatic {
            ui.set_status(tr("Nothing to compress yet – the chat is still short.").into());
        }
        return;
    };
    let model = ui.get_selected_model().to_string();
    if model.is_empty() || cloud_cli::from_model(&model).is_some() {
        if !automatic {
            ui.set_status(tr("Choose a local or server model to compress the chat.").into());
        }
        return;
    }
    let current = settings::load();
    let context = current.context_for(&model);
    let text = transcript(&all, from, to, (context as usize * 3).saturating_sub(3000).max(4000));
    let chat_id = ui.get_current_chat_id().to_string();
    let count = messages.len();
    ui.set_busy(true);
    ui.set_mic_status(tr("🗜 Compressing the chat…").into());
    let weak = ui.as_weak();
    let keep_alive = current.keep_alive.clone();
    thread::spawn(move || {
        let result = summarize(&model, context, &keep_alive, text);
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.set_busy(false);
            ui.set_mic_status("".into());
            let summary = match result {
                Ok(summary) => summary,
                Err(e) => {
                    ui.set_status(trf("Compressing failed: {}", &[&e]).into());
                    return;
                }
            };
            let list = chat::message_list();
            if ui.get_current_chat_id() != chat_id.as_str() || list.row_count() != count {
                return;
            }
            let compressed = to - from;
            let info = trf("{} older messages summarized", &[&compressed]);
            list.insert(to, chat::chat_message(false, &block(&summary), "", STATUS, &info));
            crate::checkpoint::shift(&chat_id, to, 1);
            let messages: Vec<ChatMessage> = list.iter().collect();
            let tokens = ollama::estimate_tokens(&chat::history(messages.into_iter()));
            let fresh = chat::context_info(tokens, context as u64, true);
            let last = (0..list.row_count()).rev().find(|index| list.row_data(*index).is_some_and(|m| !m.is_user));
            if let Some(last) = last
                && let Some(mut message) = list.row_data(last)
            {
                message.info = replace_context(&message.info, &fresh).into();
                list.set_row_data(last, message);
            }
            chat::update_context_bar(&ui);
            chat::mark_checkpoints(&ui);
            chat::save_current_chat(&ui);
            ui.set_status(trf("🗜 Chat compressed: {} older messages summarized.", &[&compressed]).into());
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(is_user: bool, text: &str) -> Part {
        Part { is_user, text: text.to_string(), status: String::new() }
    }

    #[test]
    fn range_keeps_the_newest() {
        let short: Vec<Part> = (0..4).map(|i| part(i % 2 == 0, "x")).collect();
        assert_eq!(range(&short), None);
        let long: Vec<Part> = (0..10).map(|i| part(i % 2 == 0, "x")).collect();
        assert_eq!(range(&long), Some((0, 6)));
        let mut again: Vec<Part> = (0..10).map(|i| part(i % 2 == 0, "x")).collect();
        again.insert(6, Part { is_user: false, text: block("old"), status: STATUS.to_string() });
        assert_eq!(range(&again), None);
        again.extend((0..4).map(|i| part(i % 2 == 0, "y")));
        assert_eq!(range(&again), Some((7, 11)));
        let text = transcript(&again, 7, 11, 10_000);
        assert!(text.starts_with("Earlier summary:\nold"), "{text}");
        assert!(text.contains("User: x") && text.contains("Assistant: x"));
    }

    #[test]
    fn transcript_drops_oldest_when_too_long() {
        let parts = vec![part(true, &"a".repeat(500)), part(false, "b"), part(true, "c")];
        let text = transcript(&parts, 0, 3, 100);
        assert!(!text.contains("aaa") && text.contains("[…]") && text.contains("User: c"));
    }

    #[test]
    fn summary_round_trip_and_info() {
        assert_eq!(summary_of(&block("- one\n- two")), "- one\n- two");
        assert_eq!(replace_context("12.5k / 16.0k tokens context", "~3.1k / 16.0k tokens context"), "~3.1k / 16.0k tokens context");
        assert_eq!(replace_context("⏸ wait · ~1.0k / 8.0k tokens context", "~2 / 8.0k tokens context"), "⏸ wait · ~2 / 8.0k tokens context");
        assert_eq!(replace_context("", "x"), "x");
    }
}
