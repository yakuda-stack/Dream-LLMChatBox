use crate::i18n::{tr, trf};
use crate::ollama::{ChatOutcome, Message};
use crate::sandbox::Folder;
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const MARK: &str = "☁ ";
const TIMEOUT: Duration = Duration::from_secs(900);
const CLAUDE_INSTALLER: &str = "curl -fsSL https://claude.ai/install.sh | bash";
const GROK_INSTALLER: &str = "curl -fsSL https://x.ai/cli/install.sh | bash";

pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub binary: &'static str,
    pub npm: Option<&'static str>,
    pub login: &'static [&'static str],
    pub login_files: &'static [&'static str],
    pub login_env: &'static [&'static str],
}

pub const PROVIDERS: [Provider; 4] = [
    Provider {
        id: "claude",
        name: "Claude Code",
        binary: "claude",
        npm: None,
        login: &["claude"],
        login_files: &[".claude/.credentials.json"],
        login_env: &["ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_AUTH_TOKEN"],
    },
    Provider {
        id: "chatgpt",
        name: "ChatGPT (Codex)",
        binary: "codex",
        npm: Some("@openai/codex"),
        login: &["codex", "login"],
        login_files: &[".codex/auth.json"],
        login_env: &["OPENAI_API_KEY"],
    },
    Provider {
        id: "gemini",
        name: "Gemini",
        binary: "gemini",
        npm: Some("@google/gemini-cli"),
        login: &["gemini"],
        login_files: &[".gemini/oauth_creds.json"],
        login_env: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
    },
    Provider {
        id: "grok",
        name: "Grok",
        binary: "grok",
        npm: None,
        login: &["grok", "login"],
        login_files: &[],
        login_env: &["XAI_API_KEY", "GROK_CODE_XAI_API_KEY"],
    },
];

const LOGIN_HINTS: [&str; 8] = [
    "/login",
    "invalid api key",
    "not logged in",
    "please log in",
    "please login",
    "codex login",
    "authentication required",
    "native binary not installed",
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|provider| provider.id == id)
}

pub fn display_name(provider: &Provider) -> String {
    format!("{MARK}{}", provider.name)
}

pub fn from_model(model: &str) -> Option<&'static Provider> {
    let name = model.strip_prefix(MARK)?;
    PROVIDERS.iter().find(|provider| provider.name == name)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn is_exe(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

pub fn find_binary(name: &str) -> Option<PathBuf> {
    let home = home();
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    dirs.push(home.join(".local/bin"));
    dirs.push(home.join(".npm-global/bin"));
    dirs.push(home.join(".grok/bin"));
    if let Ok(entries) = std::fs::read_dir(home.join(".nvm/versions/node")) {
        let mut versions: Vec<PathBuf> = entries.flatten().map(|entry| entry.path().join("bin")).collect();
        versions.sort();
        dirs.extend(versions.into_iter().rev());
    }
    dirs.into_iter().map(|dir| dir.join(name)).find(|path| is_exe(path))
}

pub fn installed(provider: &Provider) -> bool {
    find_binary(provider.binary).is_some()
}

pub fn logged_in(provider: &Provider) -> Option<bool> {
    if provider.login_env.iter().any(|var| std::env::var_os(var).is_some_and(|v| !v.is_empty())) {
        return Some(true);
    }
    let home = home();
    if provider.login_files.iter().any(|file| home.join(file).is_file()) {
        return Some(true);
    }
    if provider.id == "gemini"
        && std::fs::read_to_string(home.join(".gemini/settings.json"))
            .is_ok_and(|text| text.contains("selectedType") || text.contains("selectedAuthType"))
    {
        return Some(true);
    }
    if provider.id == "chatgpt" || provider.id == "grok" { None } else { Some(false) }
}

pub fn install_command(provider: &Provider) -> Result<Vec<String>, String> {
    let has = |name: &str| find_binary(name).is_some() || which(name);
    match provider.id {
        "claude" if has("curl") => Ok(vec!["bash".into(), "-c".into(), CLAUDE_INSTALLER.into()]),
        "grok" if has("curl") => Ok(vec!["bash".into(), "-c".into(), GROK_INSTALLER.into()]),
        "claude" | "grok" => Err(tr("curl is missing – install curl first")),
        _ => {
            let package = provider.npm.ok_or("no package")?;
            if !has("npm") {
                return Err(tr("npm is missing – install Node.js (npm) first, e.g. sudo pacman -S npm"));
            }
            Ok(vec![
                "npm".into(),
                "install".into(),
                "-g".into(),
                "--prefix".into(),
                home().join(".local").to_string_lossy().to_string(),
                "--include=optional".into(),
                "--ignore-scripts=false".into(),
                package.into(),
            ])
        }
    }
}

fn which(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|dir| is_exe(&dir.join(name))))
}

pub fn login_command(provider: &Provider) -> Option<Vec<String>> {
    let exe = find_binary(provider.binary)?;
    let mut argv = vec![exe.to_string_lossy().to_string()];
    argv.extend(provider.login.iter().skip(1).map(|part| part.to_string()));
    Some(argv)
}

pub fn prompt_from(request: &[Message]) -> String {
    let mut out = String::new();
    for message in request {
        let text = message.content.trim();
        if text.is_empty() || message.role == "tool" {
            continue;
        }
        let who = match message.role.as_str() {
            "system" => "Instructions",
            "user" => "User",
            _ => "Assistant",
        };
        out.push_str(&format!("## {who}\n{text}\n\n"));
    }
    out.push_str("Answer the last user message.");
    out
}

pub fn command(provider: &Provider, exe: &Path, prompt: &str, last_file: &Path) -> (Vec<String>, Option<String>) {
    let exe = exe.to_string_lossy().to_string();
    let s = |value: &str| value.to_string();
    match provider.id {
        "claude" => (vec![exe, s("-p"), s(prompt)], None),
        "gemini" => (vec![exe, s("-p"), s(prompt), s("--output-format"), s("json")], None),
        "grok" => (
            vec![exe, s("-p"), s(prompt), s("--output-format"), s("plain"), s("--no-auto-update")],
            None,
        ),
        _ => (
            vec![
                exe,
                s("exec"),
                s("--skip-git-repo-check"),
                s("--sandbox"),
                s("read-only"),
                s("--output-last-message"),
                last_file.to_string_lossy().to_string(),
                s("-"),
            ],
            Some(prompt.to_string()),
        ),
    }
}

pub fn clean(text: &str) -> String {
    let ansi = regex::Regex::new(r"\x1b\[[0-9;?]*[A-Za-z]").expect("valid regex");
    ansi.replace_all(text, "").trim().to_string()
}

pub fn gemini_answer(output: &str) -> Result<String, String> {
    let Some(start) = output.find('{') else { return Ok(output.to_string()) };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&output[start..]) else {
        return Ok(output.to_string());
    };
    let answer = value["response"].as_str().unwrap_or_default();
    if !value["error"].is_null() && answer.is_empty() {
        let message = value["error"]["message"].as_str().map(String::from).unwrap_or_else(|| value["error"].to_string());
        return Err(format!("Gemini: {message}"));
    }
    Ok(answer.trim().to_string())
}

fn work_dir(folders: &[Folder]) -> PathBuf {
    if let Some(folder) = folders.first() {
        return PathBuf::from(&folder.path);
    }
    let dir = std::env::temp_dir().join("dream-llmchatbox-online");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn chat(
    provider: &Provider,
    request: &[Message],
    folders: &[Folder],
    cancel: &Arc<AtomicBool>,
    mut on_chunk: impl FnMut(&str, bool),
) -> Result<ChatOutcome, String> {
    let exe = find_binary(provider.binary)
        .ok_or_else(|| trf("{} is not installed – Models → Online → Install", &[&provider.name]))?;
    let last_file = std::env::temp_dir().join(format!("dream-online-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&last_file);
    let (argv, stdin) = command(provider, &exe, &prompt_from(request), &last_file);
    let mut cmd = Command::new(&argv[0]);
    if let Some(dir) = exe.parent() {
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut parts = vec![dir.to_path_buf()];
        parts.extend(std::env::split_paths(&old));
        if let Ok(path) = std::env::join_paths(parts) {
            cmd.env("PATH", path);
        }
    }
    cmd.args(&argv[1..])
        .current_dir(work_dir(folders))
        .env("NO_COLOR", "1")
        .env("GEMINI_CLI_TRUST_WORKSPACE", "true")
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = cmd.spawn().map_err(|e| trf("Could not start {}: {}", &[&provider.binary, &e]))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = pipe.write_all(text.as_bytes());
        });
    }
    let mut out_pipe = child.stdout.take().ok_or("no output")?;
    let mut err_pipe = child.stderr.take().ok_or("no output")?;
    let out_thread = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = out_pipe.read_to_string(&mut text);
        text
    });
    let err_thread = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = err_pipe.read_to_string(&mut text);
        text
    });
    let started = Instant::now();
    let status = loop {
        if let Ok(Some(status)) = child.try_wait() {
            break Some(status);
        }
        if cancel.load(Ordering::SeqCst) || started.elapsed() > TIMEOUT {
            unsafe { libc_kill(child.id()) };
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    let out = clean(&out_thread.join().unwrap_or_default());
    let err = clean(&err_thread.join().unwrap_or_default());
    let Some(status) = status else {
        if cancel.load(Ordering::SeqCst) {
            return Ok(ChatOutcome { stats: None, tool_calls: Vec::new() });
        }
        return Err(trf("{} took longer than 15 minutes", &[&provider.name]));
    };
    let mut answer = std::fs::read_to_string(&last_file).map(|text| clean(&text)).unwrap_or_default();
    let _ = std::fs::remove_file(&last_file);
    if answer.is_empty() {
        answer = if provider.id == "gemini" { gemini_answer(&out)? } else { out };
    }
    let low = format!("{answer}\n{err}").to_lowercase();
    if answer.len() < 400 && LOGIN_HINTS.iter().any(|hint| low.contains(hint)) {
        return Err(trf("{} is not logged in – Models → Online → Log in", &[&provider.name]));
    }
    if answer.is_empty() {
        let detail: String = err.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
        return Err(if status.success() {
            trf("{} gave no answer", &[&provider.name])
        } else {
            trf("{} failed: {}", &[&provider.name, &detail])
        });
    }
    on_chunk(&answer, false);
    Ok(ChatOutcome { stats: None, tool_calls: Vec::new() })
}

unsafe fn libc_kill(pid: u32) {
    let _ = Command::new("kill").args(["-TERM", &format!("-{pid}")]).status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_prompts_and_commands() {
        let claude = provider("claude").unwrap();
        assert_eq!(from_model(&display_name(claude)).unwrap().id, "claude");
        assert!(from_model("qwen3:8b").is_none());
        let request = vec![
            Message { role: "system".into(), content: "Be short.".into(), ..Default::default() },
            Message { role: "user".into(), content: "Hi".into(), ..Default::default() },
        ];
        let prompt = prompt_from(&request);
        assert!(prompt.contains("## Instructions\nBe short.") && prompt.contains("## User\nHi"));
        let (argv, stdin) = command(provider("chatgpt").unwrap(), Path::new("/x/codex"), "p", Path::new("/tmp/l"));
        assert_eq!(argv[1], "exec");
        assert_eq!(stdin.as_deref(), Some("p"));
        assert_eq!(gemini_answer("log\n{\"response\":\" ok \"}").unwrap(), "ok");
        assert!(gemini_answer("{\"response\":\"\",\"error\":{\"message\":\"quota\"}}").unwrap_err().contains("quota"));
        assert_eq!(clean("\x1b[31mred\x1b[0m"), "red");
    }
}
