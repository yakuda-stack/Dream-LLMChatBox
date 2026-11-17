use crate::i18n::{tr, trf};
use crate::rag;
use crate::settings;
use crate::AppWindow;
use serde_json::{Value, json};
use slint::ComponentHandle;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const TOOL: &str = "search_documents";
const RESULTS: usize = 6;

pub fn model() -> String {
    let model = settings::load().embed_model;
    if model.trim().is_empty() { rag::DEFAULT_MODEL.to_string() } else { model.trim().to_string() }
}

pub fn with_docs_tool(mut tools: Value, has_docs: bool) -> Value {
    if has_docs && let Some(list) = tools.as_array_mut() {
        list.push(json!({
            "type": "function",
            "function": {
                "name": TOOL,
                "description": "Search the group's document folder (PDF, DOCX, ODT, Markdown, text, code) by meaning and get the best matching passages with file name and line. Use it before answering questions about these documents.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "What you are looking for, in a few words or a question" }
                    },
                    "required": ["query"]
                }
            }
        }));
    }
    tools
}

pub fn run_tool(folder: Option<&str>, arguments: &Value) -> String {
    let Some(folder) = folder else {
        return "Error: this group has no document folder".to_string();
    };
    let query = arguments["query"].as_str().unwrap_or_default().trim();
    if query.is_empty() {
        return "Error: query is empty".to_string();
    }
    rag::search(Path::new(folder), &model(), query, RESULTS).unwrap_or_else(|e| format!("Error: {e}"))
}

pub fn status_text(folder: &str) -> String {
    if folder.trim().is_empty() {
        return String::new();
    }
    match rag::load(Path::new(folder)) {
        Some(index) => trf("Indexed: {} files, {} passages ({})", &[&index.files.len(), &index.chunks.len(), &index.model]),
        None => tr("Not indexed yet – press “Index now”"),
    }
}

pub fn index_now(ui: &AppWindow, folder: PathBuf) {
    let weak = ui.as_weak();
    let model = model();
    ui.set_group_dialog_docs_status(tr("Indexing… (first time can take a while)").into());
    std::thread::spawn(move || {
        let progress_weak = weak.clone();
        let progress = move |done: usize, total: usize| {
            let text = trf("Indexing… {} / {} files", &[&done, &total]);
            let _ = progress_weak.upgrade_in_event_loop(move |ui| ui.set_group_dialog_docs_status(text.into()));
        };
        let result = rag::update(&folder, &model, None::<Duration>, &progress);
        let _ = weak.upgrade_in_event_loop(move |ui| {
            let text = match result {
                Ok(stats) => trf("Indexed: {} files, {} passages ({} new or changed)", &[&stats.files, &stats.chunks, &stats.changed]),
                Err(e) => rag::explain(&e, &model),
            };
            ui.set_group_dialog_docs_status(text.into());
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_tool_only_with_folder() {
        let base = json!([{ "type": "function", "function": { "name": "x" } }]);
        assert_eq!(with_docs_tool(base.clone(), false).as_array().unwrap().len(), 1);
        assert_eq!(with_docs_tool(base, true)[1]["function"]["name"], TOOL);
        assert!(run_tool(None, &json!({"query": "a"})).starts_with("Error"));
    }
}
