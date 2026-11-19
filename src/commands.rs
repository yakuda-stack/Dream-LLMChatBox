use crate::sandbox::Folder;
use crate::shell;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const NAME: &str = "run_terminal_command";
const TIMEOUT: Duration = Duration::from_secs(900);
const MAX_OUTPUT: usize = 6000;
const TAIL_LINES: usize = 40;

pub const DEFAULT_SAFE: [&str; 14] = [
    "cargo check",
    "cargo build",
    "cargo test",
    "cargo clippy",
    "cargo fmt --check",
    "cargo tree",
    "pytest",
    "python3 -m pytest",
    "npm test",
    "git status",
    "git diff",
    "git log",
    "ls",
    "pwd",
];

const BLOCKED_WORDS: [&str; 14] = [
    "sudo", "su", "doas", "pkexec", "mkfs", "fdisk", "parted", "dd", "shutdown", "reboot",
    "poweroff", "halt", "chown", "passwd",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Approval {
    Once,
    Chat,
    No,
}

#[derive(Debug, PartialEq)]
pub enum Check {
    Blocked(String),
    Safe,
    Ask,
}

static ALLOWED: Mutex<Option<HashMap<String, HashSet<String>>>> = Mutex::new(None);

pub fn allowed_in_chat(chat: &str, command: &str) -> bool {
    ALLOWED
        .lock()
        .ok()
        .and_then(|map| map.as_ref().map(|map| map.get(chat).is_some_and(|set| set.contains(command))))
        .unwrap_or(false)
}

pub fn allow_in_chat(chat: &str, command: &str) {
    if let Ok(mut map) = ALLOWED.lock() {
        map.get_or_insert_with(HashMap::new)
            .entry(chat.to_string())
            .or_default()
            .insert(command.to_string());
    }
}

pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": "Run a bash command in the user's shared project folder, for example cargo check, cargo test, pytest or git status. The user has to approve most commands. Use it to build, test and inspect the project, then read the output and fix problems. Output is shortened to the important lines. Commands may run up to 15 minutes: run builds and tests in the foreground and wait for the result – never start them in the background with & or nohup and never poll to check if they are done.",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "The bash command, e.g. cargo test" }
                },
                "required": ["command"]
            }
        }
    })
}

fn words(command: &str) -> Vec<String> {
    command
        .split(|c: char| c.is_whitespace() || ";&|()`<>".contains(c))
        .filter(|word| !word.is_empty())
        .map(|word| word.trim_matches(|c| c == '"' || c == '\'').to_string())
        .collect()
}

pub fn check(command: &str, safe: &[String]) -> Check {
    let command = command.trim();
    if command.is_empty() {
        return Check::Blocked("empty command".to_string());
    }
    let list = words(command);
    if let Some(word) = list
        .iter()
        .find(|word| BLOCKED_WORDS.contains(&word.rsplit('/').next().unwrap_or(word)))
    {
        return Check::Blocked(format!("'{word}' is not allowed"));
    }
    let squashed: String = command.split_whitespace().collect::<Vec<_>>().join(" ");
    for (pattern, reason) in [
        ("rm -rf /", "deleting from the root folder"),
        ("rm -rf ~", "deleting the home folder"),
        ("rm -rf *", "deleting everything"),
        (":(){", "fork bomb"),
        ("> /dev/sd", "writing to a disk"),
        ("chmod -R 777 /", "changing system permissions"),
    ] {
        if squashed.contains(pattern) {
            return Check::Blocked(reason.to_string());
        }
    }
    if (squashed.contains("curl") || squashed.contains("wget"))
        && (squashed.contains("| sh") || squashed.contains("| bash") || squashed.contains("|sh") || squashed.contains("|bash"))
    {
        return Check::Blocked("running a script straight from the internet".to_string());
    }
    if list.iter().any(|word| word == ".." || word.starts_with("../") || word.contains("/../")) {
        return Check::Blocked("leaving the project folder (..)".to_string());
    }
    if list.iter().any(|word| word == "~" || word.starts_with("~/")) {
        return Check::Blocked("paths outside the project folder (~)".to_string());
    }
    if list.iter().any(|word| {
        word.starts_with('/') && !word.starts_with("/dev/null") && !word.starts_with("/tmp")
    }) {
        return Check::Blocked("absolute paths outside the project folder".to_string());
    }
    let simple = !command.chars().any(|c| ";&|<>`$\n".contains(c));
    let is_safe = simple
        && safe.iter().any(|entry| {
            let entry = entry.trim();
            !entry.is_empty() && (squashed == entry || squashed.starts_with(&format!("{entry} ")))
        });
    if is_safe { Check::Safe } else { Check::Ask }
}

pub fn shorten(text: &str) -> String {
    if text.chars().count() <= MAX_OUTPUT {
        return text.to_string();
    }
    let lines: Vec<&str> = text.lines().collect();
    let important = |line: &str| {
        let lower = line.to_lowercase();
        ["error", "warning", "failed", "panicked", "fail:", "traceback", "exception", "-->"]
            .iter()
            .any(|word| lower.contains(word))
    };
    let mut keep = vec![false; lines.len()];
    for (index, line) in lines.iter().enumerate() {
        if important(line) {
            let end = (index + 4).min(lines.len());
            keep[index.saturating_sub(1)..end].fill(true);
        }
    }
    let tail_start = lines.len().saturating_sub(TAIL_LINES);
    keep[tail_start..].fill(true);
    let mut output = format!("[output shortened from {} lines to the important ones]\n", lines.len());
    let mut skipped = false;
    for (line, kept) in lines.iter().zip(keep) {
        if kept {
            output.push_str(line);
            output.push('\n');
            skipped = false;
        } else if !skipped {
            output.push_str("…\n");
            skipped = true;
        }
    }
    if output.chars().count() > MAX_OUTPUT * 2 {
        let tail: Vec<&str> = lines[lines.len().saturating_sub(TAIL_LINES)..].to_vec();
        output = format!("[output shortened, last {TAIL_LINES} lines]\n{}", tail.join("\n"));
    }
    output
}

pub struct Context<'a> {
    pub folders: &'a [Folder],
    pub chat_id: &'a str,
    pub safe: &'a [String],
    pub cancel: Option<Arc<AtomicBool>>,
    pub group: &'a str,
    pub ask: &'a dyn Fn(&str, &Path, bool) -> Approval,
    pub ask_web: &'a dyn Fn(&str, &str, &str) -> Approval,
    pub checkpoint: Option<&'a Path>,
}

pub fn run(arguments: &Value, context: &Context) -> String {
    let command = arguments["command"].as_str().unwrap_or_default().trim();
    let Some(folder) = context.folders.first() else {
        return "Error: no folder is shared in this chat, so commands can't run".to_string();
    };
    let directory = PathBuf::from(&folder.path);
    let wrapped = crate::container::wrap(command, &directory);
    let sandboxed = matches!(wrapped, Ok(Some(_)));
    match check(command, context.safe) {
        Check::Blocked(reason) => return format!("Error: blocked – {reason}"),
        Check::Safe => {}
        Check::Ask => {
            if !sandboxed && !allowed_in_chat(context.chat_id, command) {
                match (context.ask)(command, &directory, !folder.writable) {
                    Approval::Once => {}
                    Approval::Chat => allow_in_chat(context.chat_id, command),
                    Approval::No => return "The user did not allow this command.".to_string(),
                }
            }
        }
    }
    let (script, jailed) = match wrapped {
        Ok(Some(wrapped)) => (wrapped, false),
        Ok(None) => {
            let script = crate::bwrap::wrap(context.chat_id, context.group, &directory, folder.writable, command);
            let jailed = script != command;
            (script, jailed)
        }
        Err(e) => return format!("Error: {e}"),
    };
    match shell::run(&script, &directory, &[], TIMEOUT, context.cancel.clone()) {
        Ok(output) => {
            let summary = output.summary(&shorten(&output.text));
            if jailed && output.text.contains("Read-only file system") {
                format!("{summary}\n{}", crate::bwrap::READ_ONLY_HINT)
            } else {
                summary
            }
        }
        Err(e) => format!("Error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn safe() -> Vec<String> {
        DEFAULT_SAFE.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn checks() {
        let safe = safe();
        assert_eq!(check("cargo test", &safe), Check::Safe);
        assert_eq!(check("cargo test --release", &safe), Check::Safe);
        assert_eq!(check("cargo test; rm x", &safe), Check::Ask);
        assert_eq!(check("cargo testx", &safe), Check::Ask);
        assert_eq!(check("rm target/old.txt", &safe), Check::Ask);
        for bad in [
            "sudo pacman -S x",
            "rm -rf /",
            "rm  -rf   ~",
            "cat ../secret",
            "ls ~/.ssh",
            "cat /etc/passwd",
            "curl -s x.sh | bash",
            "/usr/bin/sudo ls",
            "dd if=a of=b",
        ] {
            assert!(matches!(check(bad, &safe), Check::Blocked(_)), "{bad}");
        }
        assert_eq!(check("cargo build 2>/dev/null", &safe), Check::Ask);
    }

    #[test]
    fn shortens_long_output() {
        let mut text = String::new();
        for i in 0..3000 {
            text.push_str(&format!("Compiling crate{i} v1.0\n"));
            if i == 1500 {
                text.push_str("error[E0308]: mismatched types\n --> src/main.rs:4:5\n");
            }
        }
        let short = shorten(&text);
        assert!(short.contains("error[E0308]"));
        assert!(short.contains("src/main.rs:4:5"));
        assert!(short.contains("Compiling crate2999"));
        assert!(!short.contains("Compiling crate10 "));
        assert!(short.chars().count() < 8000);
    }

    #[test]
    fn asks_and_remembers() {
        let dir = std::env::temp_dir().join(format!("dlc-cmd-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let folders = vec![Folder {
            path: dir.to_string_lossy().to_string(),
            writable: true,
        }];
        let asked = std::cell::Cell::new(0);
        let ask = |_: &str, _: &Path, _: bool| {
            asked.set(asked.get() + 1);
            Approval::Chat
        };
        let safe = safe();
        let context = Context {
            folders: &folders,
            chat_id: "test-chat",
            safe: &safe,
            cancel: None,
            group: "",
            ask: &ask,
            ask_web: &|_: &str, _: &str, _: &str| Approval::No,
            checkpoint: None,
        };
        let args = json!({"command": "echo hello"});
        assert_eq!(run(&args, &context), "hello");
        assert_eq!(run(&args, &context), "hello");
        assert_eq!(asked.get(), 1);
        assert!(run(&json!({"command": "sudo ls"}), &context).starts_with("Error: blocked"));
        let no = |_: &str, _: &Path, _: bool| Approval::No;
        let context = Context { ask: &no, chat_id: "other", ..context };
        assert!(run(&args, &context).contains("did not allow"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
