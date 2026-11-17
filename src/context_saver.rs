use crate::ollama::{Message, ToolCall};
use crate::sandbox::{self, Folder};
use std::collections::HashMap;
use std::fs;
use std::time::SystemTime;

pub const SHORTENED: &str = "[Shortened to save context:";
const KEEP_CHARS: usize = 300;

#[derive(Default)]
pub struct Reads {
    seen: HashMap<String, (Option<SystemTime>, usize)>,
}

fn key(call: &ToolCall) -> Option<String> {
    (call.function.name == "read_file").then(|| format!("{}", call.function.arguments))
}

fn changed_at(call: &ToolCall, folders: &[Folder]) -> Option<SystemTime> {
    let path = call.function.arguments["path"].as_str()?;
    let resolved = sandbox::resolve(folders, path).ok()?;
    fs::metadata(resolved.path).ok()?.modified().ok()
}

impl Reads {
    pub fn already_read(&self, call: &ToolCall, folders: &[Folder], request: &[Message]) -> Option<String> {
        let (time, index) = self.seen.get(&key(call)?)?;
        let still_there = request
            .get(*index)
            .is_some_and(|message| message.role == "tool" && !message.content.starts_with(SHORTENED));
        let unchanged = time.is_some() && *time == changed_at(call, folders);
        (still_there && unchanged).then(|| {
            let path = call.function.arguments["path"].as_str().unwrap_or_default();
            format!(
                "Already read in this answer and unchanged since then: {path}. Use the earlier read_file result above instead of reading it again."
            )
        })
    }

    pub fn remember(&mut self, call: &ToolCall, folders: &[Folder], index: usize) {
        if let Some(key) = key(call) {
            self.seen.insert(key, (changed_at(call, folders), index));
        }
    }
}

pub fn shorten_old(request: &mut [Message], keep_from: usize) -> usize {
    let mut count = 0;
    for message in request.iter_mut().take(keep_from) {
        if message.role != "tool" || message.content.starts_with(SHORTENED) || message.content.len() <= KEEP_CHARS * 2 {
            continue;
        }
        let tool = message.tool_name.clone().unwrap_or_else(|| "tool".to_string());
        let lines = message.content.lines().count();
        let start: String = message.content.chars().take(KEEP_CHARS).collect();
        message.content = format!(
            "{SHORTENED} {tool} result, {lines} lines. Start:]\n{start}\n[… call the tool again if you need the rest]"
        );
        count += 1;
    }
    count
}

pub fn strip_thinking(text: &str) -> String {
    let mut rest = text;
    let mut out = String::new();
    while let Some(start) = rest.find("<think>") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</think>") {
            Some(end) => rest = &rest[start + end + "</think>".len()..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::ToolFunction;

    #[test]
    fn thinking_is_removed() {
        assert_eq!(strip_thinking("<think>hmm\nok</think>\nHello"), "Hello");
        assert_eq!(strip_thinking("A <think>x</think>B"), "A B");
        assert_eq!(strip_thinking("no think"), "no think");
    }

    #[test]
    fn old_results_are_shortened_and_reads_reused() {
        let long = "line\n".repeat(400);
        let mut request = vec![
            Message { role: "tool".into(), content: long.clone(), tool_name: Some("read_file".into()), ..Default::default() },
            Message { role: "tool".into(), content: long.clone(), ..Default::default() },
        ];
        let dir = std::env::temp_dir().join(format!("dlc-reads-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.rs"), "fn a() {}").unwrap();
        let folders = vec![Folder { path: dir.to_string_lossy().to_string(), writable: false }];
        let call = ToolCall {
            function: ToolFunction { name: "read_file".into(), arguments: serde_json::json!({ "path": "a.rs" }) },
        };
        let mut reads = Reads::default();
        reads.remember(&call, &folders, 0);
        assert!(reads.already_read(&call, &folders, &request).is_some());
        assert_eq!(shorten_old(&mut request, 1), 1);
        assert!(request[0].content.starts_with(SHORTENED));
        assert_eq!(request[1].content, long);
        assert!(reads.already_read(&call, &folders, &request).is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
