use crate::i18n::{tr, trf};
use crate::commands::{self, Approval, Context};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, LazyLock, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const PREFIX: &str = "mcp_";
const PROTOCOL: &str = "2025-06-18";
const START_TIMEOUT: Duration = Duration::from_secs(30);
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
const STDERR_LINES: usize = 30;
const MAX_OUTPUT: usize = 20000;
const MAX_PAGES: usize = 50;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerConfig {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub disabled: bool,
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
}

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dream-llmchatbox")
        .join("mcp.json")
}

pub fn claude_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Claude")
        .join("claude_desktop_config.json")
}

fn parse_server(name: &str, value: &Value) -> Option<ServerConfig> {
    let object = value.as_object()?;
    let mut extra = object.clone();
    let command = extra.remove("command").and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
    let args = extra
        .remove("args")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|arg| match arg {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .collect();
    let env = extra
        .remove("env")
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| {
            let value = match value {
                Value::String(text) => text,
                other => other.to_string(),
            };
            (key, value)
        })
        .collect();
    let disabled = extra.remove("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
    Some(ServerConfig {
        name: name.to_string(),
        command,
        args,
        env,
        disabled,
        extra,
    })
}

fn server_value(config: &ServerConfig) -> Value {
    let mut object = config.extra.clone();
    object.insert("command".into(), Value::String(config.command.clone()));
    object.insert("args".into(), json!(config.args));
    if !config.env.is_empty() || object.contains_key("env") {
        let env: Map<String, Value> = config
            .env
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect();
        object.insert("env".into(), Value::Object(env));
    }
    object.insert("disabled".into(), Value::Bool(config.disabled));
    Value::Object(object)
}

fn read_root(path: &Path) -> Result<Map<String, Value>, String> {
    match fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(trf("Could not read {}: {}", &[&path.display(), &e])),
        Ok(text) if text.trim().is_empty() => Ok(Map::new()),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => Ok(map),
            Ok(_) => Err(trf("{} is not a JSON object", &[&path.display()])),
            Err(e) => Err(trf("{} is not valid JSON: {}", &[&path.display(), &e])),
        },
    }
}

pub fn parse_servers(root: &Map<String, Value>) -> Vec<ServerConfig> {
    root.get("mcpServers")
        .and_then(|servers| servers.as_object())
        .map(|servers| {
            servers
                .iter()
                .filter_map(|(name, value)| parse_server(name, value))
                .collect()
        })
        .unwrap_or_default()
}

pub fn load_in(path: &Path) -> Result<Vec<ServerConfig>, String> {
    read_root(path).map(|root| parse_servers(&root))
}

pub fn load() -> Vec<ServerConfig> {
    load_in(&config_path()).unwrap_or_default()
}

pub fn save_in(path: &Path, servers: &[ServerConfig]) -> Result<(), String> {
    let mut root = read_root(path)?;
    let map: Map<String, Value> = servers
        .iter()
        .map(|server| (server.name.clone(), server_value(server)))
        .collect();
    root.insert("mcpServers".into(), Value::Object(map));
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?;
    fs::write(path, text + "\n").map_err(|e| trf("Could not write {}: {}", &[&path.display(), &e]))
}

pub struct Imported {
    pub added: usize,
    pub existing: usize,
    pub skipped: usize,
}

pub fn import_in(path: &Path, from: &Path) -> Result<Imported, String> {
    if !from.exists() {
        return Err(trf("No Claude Desktop config found at {}", &[&from.display()]));
    }
    let incoming = load_in(from)?;
    let mut list = load_in(path)?;
    let mut result = Imported {
        added: 0,
        existing: 0,
        skipped: 0,
    };
    for mut server in incoming {
        if server.command.trim().is_empty() {
            result.skipped += 1;
        } else if list.iter().any(|s| s.name == server.name) {
            result.existing += 1;
        } else {
            server.disabled = true;
            list.push(server);
            result.added += 1;
        }
    }
    if result.added > 0 {
        save_in(path, &list)?;
    }
    Ok(result)
}

pub fn split_args(text: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => current.push(c),
                        None => return Err(tr("missing closing ' in the arguments")),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\')) => current.push(c),
                            Some(c) => {
                                current.push('\\');
                                current.push(c);
                            }
                            None => return Err(tr("missing closing \" in the arguments")),
                        },
                        Some(c) => current.push(c),
                        None => return Err(tr("missing closing \" in the arguments")),
                    }
                }
            }
            '\\' => {
                started = true;
                if let Some(c) = chars.next() {
                    current.push(c);
                }
            }
            c => {
                started = true;
                current.push(c);
            }
        }
    }
    if started {
        args.push(current);
    }
    Ok(args)
}

pub fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            let plain = !arg.is_empty()
                && !arg.chars().any(|c| c.is_whitespace() || "\"'\\".contains(c));
            if plain {
                arg.clone()
            } else {
                format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn parse_env(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut list = Vec::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((key, value)) = line.split_once('=') else {
            return Err(trf("\"{}\" is not KEY=value", &[&line]));
        };
        let key = key.trim();
        if key.is_empty() || key.chars().any(char::is_whitespace) {
            return Err(trf("\"{}\" has no valid name before =", &[&line]));
        }
        list.push((key.to_string(), value.to_string()));
    }
    Ok(list)
}

pub fn env_text(env: &[(String, String)]) -> String {
    env.iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn expand_home(arg: &str) -> String {
    match (arg.strip_prefix('~'), dirs::home_dir()) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            format!("{}{rest}", home.display())
        }
        _ => arg.to_string(),
    }
}

pub fn sanitize(server: &str, tool: &str) -> String {
    let raw = format!("{PREFIX}{server}_{tool}").to_lowercase();
    let mut name: String = raw
        .chars()
        .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { '_' })
        .collect();
    while name.contains("__") {
        name = name.replace("__", "_");
    }
    let name = name.trim_end_matches('_');
    name.chars().take(64).collect()
}

type Pending = Arc<Mutex<HashMap<u64, Sender<Result<Value, String>>>>>;

pub struct Server {
    pub name: String,
    child: Mutex<Option<Child>>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    pending: Pending,
    next_id: AtomicU64,
    stderr: Arc<Mutex<VecDeque<String>>>,
    alive: Arc<AtomicBool>,
    pub tools: Vec<McpTool>,
}

fn write_line(stdin: &Mutex<Option<ChildStdin>>, message: &Value) -> Result<(), String> {
    let mut guard = stdin.lock().map_err(|_| "the server pipe is broken".to_string())?;
    let pipe = guard.as_mut().ok_or("the server is stopped")?;
    let mut line = message.to_string();
    line.push('\n');
    pipe.write_all(line.as_bytes())
        .and_then(|_| pipe.flush())
        .map_err(|e| format!("could not write to the server: {e}"))
}

fn route(line: &str, pending: &Pending, stdin: &Mutex<Option<ChildStdin>>) {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return;
    };
    let id = message.get("id").filter(|id| !id.is_null());
    if let Some(method) = message.get("method").and_then(Value::as_str) {
        if let Some(id) = id {
            let reply = if method == "ping" {
                json!({"jsonrpc": "2.0", "id": id, "result": {}})
            } else {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "method not found"}})
            };
            let _ = write_line(stdin, &reply);
        }
        return;
    }
    let Some(number) = id.and_then(Value::as_u64) else {
        return;
    };
    let sender = pending.lock().ok().and_then(|mut map| map.remove(&number));
    if let Some(sender) = sender {
        let result = match message.get("error") {
            Some(error) => Err(error["message"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| error.to_string())),
            None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = sender.send(result);
    }
}

fn kill_group(pid: u32, signal: &str) {
    let _ = Command::new("kill")
        .args([signal, "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

impl Server {
    pub fn start(config: &ServerConfig, timeout: Duration) -> Result<Server, String> {
        let mut command = Command::new(&config.command);
        command
            .args(config.args.iter().map(|arg| expand_home(arg)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        for (key, value) in &config.env {
            command.env(key, value);
        }
        if let Some(home) = dirs::home_dir() {
            command.current_dir(home);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("could not start '{}': {e}", config.command))?;
        let stdin = Arc::new(Mutex::new(child.stdin.take()));
        let stdout = child.stdout.take();
        let stderr_pipe = child.stderr.take();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let alive = Arc::new(AtomicBool::new(true));
        let stderr = Arc::new(Mutex::new(VecDeque::new()));
        {
            let pending = pending.clone();
            let alive = alive.clone();
            let stdin = stdin.clone();
            thread::spawn(move || {
                if let Some(pipe) = stdout {
                    for line in BufReader::new(pipe).lines() {
                        let Ok(line) = line else { break };
                        route(&line, &pending, &stdin);
                    }
                }
                alive.store(false, Ordering::SeqCst);
                if let Ok(mut map) = pending.lock() {
                    map.clear();
                }
            });
        }
        {
            let stderr = stderr.clone();
            thread::spawn(move || {
                let Some(pipe) = stderr_pipe else { return };
                for line in BufReader::new(pipe).lines() {
                    let Ok(line) = line else { break };
                    if let Ok(mut lines) = stderr.lock() {
                        lines.push_back(line);
                        while lines.len() > STDERR_LINES {
                            lines.pop_front();
                        }
                    }
                }
            });
        }
        let mut server = Server {
            name: config.name.clone(),
            child: Mutex::new(Some(child)),
            stdin,
            pending,
            next_id: AtomicU64::new(1),
            stderr,
            alive,
            tools: Vec::new(),
        };
        match server.handshake(timeout) {
            Ok(tools) => {
                server.tools = tools;
                Ok(server)
            }
            Err(e) => {
                let message = if server.alive() {
                    let tail = server.stderr_tail(3);
                    if tail.is_empty() { e } else { format!("{e} – {tail}") }
                } else {
                    thread::sleep(Duration::from_millis(150));
                    server.stopped_message()
                };
                server.kill();
                Err(message)
            }
        }
    }

    fn handshake(&self, timeout: Duration) -> Result<Vec<McpTool>, String> {
        self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": { "name": "Dream-LLMChatBox", "version": env!("CARGO_PKG_VERSION") }
            }),
            timeout,
            None,
        )?;
        write_line(&self.stdin, &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))?;
        self.list_tools(timeout)
    }

    fn list_tools(&self, timeout: Duration) -> Result<Vec<McpTool>, String> {
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let params = match &cursor {
                Some(cursor) => json!({ "cursor": cursor }),
                None => json!({}),
            };
            let result = self.request("tools/list", params, timeout, None)?;
            for item in result["tools"].as_array().into_iter().flatten() {
                let Some(name) = item["name"].as_str() else { continue };
                let schema = match &item["inputSchema"] {
                    Value::Object(map) if !map.is_empty() => Value::Object(map.clone()),
                    _ => json!({ "type": "object", "properties": {} }),
                };
                tools.push(McpTool {
                    name: name.to_string(),
                    description: item["description"].as_str().unwrap_or_default().to_string(),
                    schema,
                });
            }
            cursor = result["nextCursor"].as_str().filter(|c| !c.is_empty()).map(str::to_string);
            if cursor.is_none() {
                break;
            }
        }
        Ok(tools)
    }

    pub fn request(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<Value, String> {
        if !self.alive() {
            return Err(self.stopped_message());
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver) = mpsc::channel();
        if let Ok(mut map) = self.pending.lock() {
            map.insert(id, sender);
        }
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        if let Err(e) = write_line(&self.stdin, &message) {
            self.forget(id);
            return Err(e);
        }
        let started = Instant::now();
        loop {
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Disconnected) => return Err(self.stopped_message()),
                Err(RecvTimeoutError::Timeout) => {}
            }
            if cancel.as_ref().is_some_and(|flag| flag.load(Ordering::SeqCst)) {
                self.forget(id);
                self.notify_cancelled(id);
                return Err("stopped by the user".to_string());
            }
            if started.elapsed() > timeout {
                self.forget(id);
                self.notify_cancelled(id);
                return Err(format!("no answer from the server after {} s", timeout.as_secs()));
            }
        }
    }

    fn forget(&self, id: u64) {
        if let Ok(mut map) = self.pending.lock() {
            map.remove(&id);
        }
    }

    fn notify_cancelled(&self, id: u64) {
        let _ = write_line(
            &self.stdin,
            &json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": id}}),
        );
    }

    pub fn call_tool(
        &self,
        tool: &str,
        arguments: &Value,
        timeout: Duration,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<String, String> {
        let arguments = match arguments {
            Value::Object(_) => arguments.clone(),
            _ => json!({}),
        };
        let result = self.request("tools/call", json!({"name": tool, "arguments": arguments}), timeout, cancel)?;
        Ok(format_result(&result))
    }

    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn stderr_tail(&self, count: usize) -> String {
        let Ok(lines) = self.stderr.lock() else {
            return String::new();
        };
        let start = lines.len().saturating_sub(count);
        lines
            .iter()
            .skip(start)
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" | ")
    }

    fn stopped_message(&self) -> String {
        let tail = self.stderr_tail(3);
        if tail.is_empty() {
            "the server stopped".to_string()
        } else {
            format!("the server stopped – {tail}")
        }
    }

    pub fn kill(&self) {
        if let Ok(mut stdin) = self.stdin.lock() {
            stdin.take();
        }
        let Ok(mut slot) = self.child.lock() else { return };
        let Some(mut child) = slot.take() else { return };
        let pid = child.id();
        kill_group(pid, "-TERM");
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(500) {
            if let Ok(Some(_)) = child.try_wait() {
                kill_group(pid, "-KILL");
                return;
            }
            thread::sleep(Duration::from_millis(20));
        }
        kill_group(pid, "-KILL");
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.kill();
    }
}

pub fn format_result(result: &Value) -> String {
    let parts: Vec<String> = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| match item["type"].as_str().unwrap_or_default() {
            "text" => item["text"].as_str().unwrap_or_default().to_string(),
            "image" => "[image]".to_string(),
            "audio" => "[audio]".to_string(),
            "resource" => match item["resource"]["text"].as_str() {
                Some(text) => text.to_string(),
                None => format!("[resource {}]", item["resource"]["uri"].as_str().unwrap_or_default()),
            },
            "resource_link" => format!("[link {}]", item["uri"].as_str().unwrap_or_default()),
            other => format!("[{other}]"),
        })
        .collect();
    let mut text = parts.join("\n");
    if text.trim().is_empty()
        && let Some(structured) = result.get("structuredContent").filter(|v| !v.is_null())
    {
        text = structured.to_string();
    }
    if text.trim().is_empty() {
        text = "(no output)".to_string();
    }
    if text.chars().count() > MAX_OUTPUT {
        text = text.chars().take(MAX_OUTPUT).collect::<String>() + "\n[output shortened]";
    }
    if result["isError"].as_bool().unwrap_or(false) {
        format!("Error: {text}")
    } else {
        text
    }
}

enum State {
    Starting,
    Running(Arc<Server>),
    Failed(String),
}

static SERVERS: LazyLock<Mutex<HashMap<String, State>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static START_LOCK: Mutex<()> = Mutex::new(());
type Listener = Box<dyn Fn() + Send>;
static LISTENER: Mutex<Option<Listener>> = Mutex::new(None);

pub fn set_listener(listener: impl Fn() + Send + 'static) {
    if let Ok(mut slot) = LISTENER.lock() {
        *slot = Some(Box::new(listener));
    }
}

fn notify() {
    if let Ok(slot) = LISTENER.lock()
        && let Some(listener) = slot.as_ref()
    {
        listener();
    }
}

fn set_state(name: &str, state: State) {
    let old = SERVERS
        .lock()
        .ok()
        .and_then(|mut map| map.insert(name.to_string(), state));
    if let Some(State::Running(server)) = old {
        server.kill();
    }
}

fn running(name: &str) -> Option<Arc<Server>> {
    let map = SERVERS.lock().ok()?;
    match map.get(name) {
        Some(State::Running(server)) => Some(server.clone()),
        _ => None,
    }
}

pub enum Status {
    NotStarted,
    Starting,
    Running(Vec<String>),
    Failed(String),
}

pub fn status(name: &str) -> Status {
    let Ok(map) = SERVERS.lock() else {
        return Status::NotStarted;
    };
    match map.get(name) {
        None => Status::NotStarted,
        Some(State::Starting) => Status::Starting,
        Some(State::Failed(e)) => Status::Failed(e.clone()),
        Some(State::Running(server)) if server.alive() => {
            Status::Running(server.tools.iter().map(|tool| tool.name.clone()).collect())
        }
        Some(State::Running(server)) => Status::Failed(server.stopped_message()),
    }
}

fn start_all(configs: &[ServerConfig], timeout: Duration) {
    for config in configs {
        set_state(&config.name, State::Starting);
    }
    notify();
    let handles: Vec<_> = configs
        .iter()
        .cloned()
        .map(|config| {
            thread::spawn(move || {
                let state = match Server::start(&config, timeout) {
                    Ok(server) => State::Running(Arc::new(server)),
                    Err(e) => State::Failed(e),
                };
                set_state(&config.name, state);
            })
        })
        .collect();
    for handle in handles {
        let _ = handle.join();
    }
    notify();
}

pub fn restart(config: &ServerConfig) -> Status {
    let _guard = START_LOCK.lock();
    stop(&config.name);
    start_all(std::slice::from_ref(config), START_TIMEOUT);
    status(&config.name)
}

fn ensure_started(configs: &[ServerConfig], timeout: Duration) {
    let _guard = START_LOCK.lock();
    let wanted: Vec<&str> = configs.iter().map(|config| config.name.as_str()).collect();
    let unwanted: Vec<String> = SERVERS
        .lock()
        .map(|map| map.keys().filter(|name| !wanted.contains(&name.as_str())).cloned().collect())
        .unwrap_or_default();
    for name in &unwanted {
        stop(name);
    }
    let missing: Vec<ServerConfig> = configs
        .iter()
        .filter(|config| match status(&config.name) {
            Status::NotStarted | Status::Starting => true,
            Status::Running(_) => false,
            Status::Failed(_) => running(&config.name).is_some(),
        })
        .cloned()
        .collect();
    if !missing.is_empty() {
        start_all(&missing, timeout);
    }
}

fn definitions_for(configs: &[ServerConfig], timeout: Duration) -> Vec<Value> {
    let enabled: Vec<ServerConfig> = configs
        .iter()
        .filter(|config| !config.disabled && !config.command.trim().is_empty())
        .cloned()
        .collect();
    ensure_started(&enabled, timeout);
    let mut seen = Vec::new();
    let mut items = Vec::new();
    for config in &enabled {
        let Some(server) = running(&config.name) else { continue };
        for tool in &server.tools {
            let name = sanitize(&server.name, &tool.name);
            if seen.contains(&name) {
                continue;
            }
            seen.push(name.clone());
            let description = format!("{} (MCP server {})", tool.description.trim(), server.name)
                .trim()
                .to_string();
            items.push(json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description,
                    "parameters": tool.schema
                }
            }));
        }
    }
    items
}

pub fn definitions() -> Vec<Value> {
    let configs = load();
    if configs.iter().all(|config| config.disabled) && SERVERS.lock().map(|m| m.is_empty()).unwrap_or(true) {
        return Vec::new();
    }
    definitions_for(&configs, START_TIMEOUT)
}

pub fn lookup(name: &str) -> Option<(Arc<Server>, String)> {
    let mut servers: Vec<Arc<Server>> = SERVERS
        .lock()
        .ok()?
        .values()
        .filter_map(|state| match state {
            State::Running(server) => Some(server.clone()),
            _ => None,
        })
        .collect();
    servers.sort_by(|a, b| a.name.cmp(&b.name));
    servers.into_iter().find_map(|server| {
        let tool = server
            .tools
            .iter()
            .find(|tool| sanitize(&server.name, &tool.name) == name)?
            .name
            .clone();
        Some((server, tool))
    })
}

pub fn label(name: &str) -> String {
    match lookup(name) {
        Some((server, tool)) => format!("MCP {}: {tool}", server.name),
        None => format!("MCP {}", name.trim_start_matches(PREFIX)),
    }
}

fn short_args(arguments: &Value) -> String {
    let text = match arguments {
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| {
                let value = match value {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                let short: String = value.chars().take(60).collect();
                if value.chars().count() > 60 {
                    format!("{key}: {short}…")
                } else {
                    format!("{key}: {short}")
                }
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    };
    if text.chars().count() > 200 {
        text.chars().take(200).collect::<String>() + "…"
    } else {
        text
    }
}

pub fn run(name: &str, arguments: &Value, context: &Context) -> Option<String> {
    if !name.starts_with(PREFIX) {
        return None;
    }
    let (server, tool) = lookup(name)?;
    let key = format!("mcp:{}:{tool}", server.name);
    if !commands::allowed_in_chat(context.chat_id, &key) {
        let text = format!("{} → {tool}({})", server.name, short_args(arguments));
        match (context.ask_web)(
            "The model wants to use an MCP tool:",
            &text,
            "\"Allow in this chat\" = all calls of this tool in this chat",
        ) {
            Approval::Once => {}
            Approval::Chat => commands::allow_in_chat(context.chat_id, &key),
            Approval::No => return Some("The user did not allow this MCP tool call.".to_string()),
        }
    }
    let result = server.call_tool(&tool, arguments, CALL_TIMEOUT, context.cancel.clone());
    if !server.alive() {
        notify();
    }
    Some(match result {
        Ok(text) => text,
        Err(e) => format!("Error: MCP server {}: {e}", server.name),
    })
}

pub fn stop(name: &str) {
    let old = SERVERS.lock().ok().and_then(|mut map| map.remove(name));
    if let Some(State::Running(server)) = old {
        server.kill();
    }
}

pub fn shutdown() {
    let all: Vec<State> = SERVERS
        .lock()
        .map(|mut map| map.drain().map(|(_, state)| state).collect())
        .unwrap_or_default();
    let handles: Vec<_> = all
        .into_iter()
        .filter_map(|state| match state {
            State::Running(server) => Some(thread::spawn(move || server.kill())),
            _ => None,
        })
        .collect();
    for handle in handles {
        let _ = handle.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE: &str = r#"
import sys, json, os, time
replies = []
def send(m):
    sys.stdout.write(json.dumps(m) + "\n")
    sys.stdout.flush()
print("starting fake", file=sys.stderr, flush=True)
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    m = json.loads(line)
    meth = m.get("method")
    i = m.get("id")
    if meth is None:
        replies.append(m)
        continue
    if i is None:
        continue
    if meth == "initialize":
        send({"jsonrpc": "2.0", "id": "srv-1", "method": "roots/list"})
        send({"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info"}})
        print("not json")
        r = {"protocolVersion": m["params"]["protocolVersion"], "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1"}}
    elif meth == "tools/list":
        if (m.get("params") or {}).get("cursor") is None:
            r = {"tools": [{"name": "echo", "description": "Echo text", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}}], "nextCursor": "p2"}
        else:
            r = {"tools": [{"name": "slow.tool", "description": "Sleeps"}, {"name": "fail"}, {"name": "env"}, {"name": "replies"}]}
    elif meth == "tools/call":
        name = m["params"]["name"]
        args = m["params"].get("arguments") or {}
        if name == "echo":
            r = {"content": [{"type": "text", "text": args.get("text", "")}, {"type": "image", "data": "", "mimeType": "image/png"}]}
        elif name == "slow.tool":
            time.sleep(10)
            r = {"content": []}
        elif name == "fail":
            r = {"content": [{"type": "text", "text": "boom"}], "isError": True}
        elif name == "env":
            r = {"content": [{"type": "text", "text": os.environ.get("FAKE_GREETING", "")}]}
        elif name == "replies":
            r = {"content": [{"type": "text", "text": json.dumps(replies)}]}
        else:
            send({"jsonrpc": "2.0", "id": i, "error": {"code": -32602, "message": "unknown tool " + name}})
            continue
    else:
        send({"jsonrpc": "2.0", "id": i, "error": {"code": -32601, "message": "nope"}})
        continue
    send({"jsonrpc": "2.0", "id": i, "result": r})
"#;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dlc-mcp-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake(tag: &str, script: &str) -> (PathBuf, ServerConfig) {
        let dir = temp_dir(tag);
        let path = dir.join("server.py");
        fs::write(&path, script).unwrap();
        let config = ServerConfig {
            name: format!("Fake {tag}"),
            command: "python3".into(),
            args: vec!["-u".into(), path.to_string_lossy().to_string()],
            env: vec![("FAKE_GREETING".into(), "hello env".into())],
            ..Default::default()
        };
        (dir, config)
    }

    #[test]
    fn start_list_call_and_stop() {
        let (dir, config) = fake("basic", FAKE);
        let server = Server::start(&config, Duration::from_secs(10)).unwrap();
        let names: Vec<&str> = server.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["echo", "slow.tool", "fail", "env", "replies"]);
        assert_eq!(server.tools[1].schema, json!({"type": "object", "properties": {}}));
        let timeout = Duration::from_secs(5);
        assert_eq!(server.call_tool("echo", &json!({"text": "hi"}), timeout, None).unwrap(), "hi\n[image]");
        assert_eq!(server.call_tool("fail", &json!({}), timeout, None).unwrap(), "Error: boom");
        assert_eq!(server.call_tool("env", &json!({}), timeout, None).unwrap(), "hello env");
        let replies = server.call_tool("replies", &json!({}), timeout, None).unwrap();
        assert!(replies.contains("srv-1") && replies.contains("-32601"), "{replies}");
        let missing = server.call_tool("nothing", &json!({}), timeout, None).unwrap_err();
        assert!(missing.contains("unknown tool"));
        let slow = server.call_tool("slow.tool", &json!({}), Duration::from_millis(400), None).unwrap_err();
        assert!(slow.contains("no answer"));
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            flag.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        let stopped = server.call_tool("slow.tool", &json!({}), timeout, Some(cancel)).unwrap_err();
        assert_eq!(stopped, "stopped by the user");
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(server.stderr_tail(3).contains("starting fake"));
        server.kill();
        assert!(server.call_tool("echo", &json!({}), timeout, None).is_err());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn errors_and_timeouts() {
        let (dir, config) = fake("crash", "import sys\nprint('bad token', file=sys.stderr, flush=True)\nsys.exit(3)\n");
        let error = Server::start(&config, Duration::from_secs(10)).err().unwrap();
        assert!(error.contains("bad token"), "{error}");
        let (dir2, config) = fake("hang", "import time\ntime.sleep(30)\n");
        let started = Instant::now();
        let error = Server::start(&config, Duration::from_millis(500)).err().unwrap();
        assert!(error.contains("no answer"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
        let missing = ServerConfig {
            name: "missing".into(),
            command: "/nonexistent/mcp-server".into(),
            ..Default::default()
        };
        assert!(Server::start(&missing, Duration::from_secs(1)).err().unwrap().contains("could not start"));
        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_dir_all(dir2);
    }

    #[test]
    fn registry_definitions_and_run() {
        let (dir, mut config) = fake("registry", FAKE);
        config.name = "Reg Server".into();
        let broken = ServerConfig {
            name: "reg-broken".into(),
            command: "/nonexistent/mcp-server".into(),
            ..Default::default()
        };
        let off = ServerConfig {
            disabled: true,
            name: "reg-off".into(),
            ..config.clone()
        };
        let items = definitions_for(&[config.clone(), broken.clone(), off], Duration::from_secs(10));
        let names: Vec<&str> = items.iter().map(|i| i["function"]["name"].as_str().unwrap()).collect();
        assert_eq!(names[0], "mcp_reg_server_echo");
        assert!(names.contains(&"mcp_reg_server_slow_tool"));
        assert_eq!(items[0]["function"]["description"], "Echo text (MCP server Reg Server)");
        assert!(matches!(status("reg-broken"), Status::Failed(_)));
        assert!(matches!(status("reg-off"), Status::NotStarted));
        assert_eq!(label("mcp_reg_server_echo"), "MCP Reg Server: echo");
        let asked = std::cell::Cell::new(0);
        let ask = |_: &str, text: &str, _: &str| {
            asked.set(asked.get() + 1);
            assert_eq!(text, "Reg Server → echo(text: yo)");
            Approval::Chat
        };
        let no = |_: &str, _: &std::path::Path, _: bool| Approval::No;
        let context = Context {
            folders: &[],
            chat_id: "mcp-chat",
            safe: &[],
            cancel: None,
            group: "",
            ask: &no,
            ask_web: &ask,
            checkpoint: None,
        };
        let args = json!({"text": "yo"});
        assert_eq!(run("mcp_reg_server_echo", &args, &context).unwrap(), "yo\n[image]");
        assert_eq!(run("mcp_reg_server_echo", &args, &context).unwrap(), "yo\n[image]");
        assert_eq!(asked.get(), 1);
        assert!(run("calculate", &args, &context).is_none());
        let refuse = |_: &str, _: &str, _: &str| Approval::No;
        let context = Context { ask_web: &refuse, chat_id: "other", ..context };
        assert!(run("mcp_reg_server_echo", &args, &context).unwrap().contains("did not allow"));
        let again = definitions_for(&[config.clone(), broken], Duration::from_secs(10));
        assert_eq!(again.len(), items.len());
        stop("Reg Server");
        stop("reg-broken");
        assert!(matches!(status("Reg Server"), Status::NotStarted));
        assert!(run("mcp_reg_server_echo", &args, &context).is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn args_env_and_names() {
        assert_eq!(
            split_args(r#"-y @mcp/server-filesystem "/home/me/My Files" 'it''s' a\ b "q\"x""#).unwrap(),
            vec!["-y", "@mcp/server-filesystem", "/home/me/My Files", "its", "a b", "q\"x"]
        );
        assert_eq!(split_args("  ").unwrap(), Vec::<String>::new());
        assert_eq!(split_args("\"\"").unwrap(), vec![""]);
        assert!(split_args("\"open").is_err());
        let args: Vec<String> = vec!["-y".into(), "a b".into(), "".into(), "q\"\\x".into()];
        assert_eq!(split_args(&join_args(&args)).unwrap(), args);
        assert_eq!(parse_env("A=1\n\n B = x=y \n").unwrap(), vec![("A".into(), "1".into()), ("B".into(), " x=y".into())]);
        assert!(parse_env("nothing").is_err());
        assert_eq!(sanitize("File System", "read-file"), "mcp_file_system_read_file");
        assert_eq!(sanitize("gh", "Get.Issue!"), "mcp_gh_get_issue");
        assert_eq!(sanitize("x", &"a".repeat(100)).len(), 64);
        let home = dirs::home_dir().unwrap().display().to_string();
        assert_eq!(expand_home("~/Documents"), format!("{home}/Documents"));
        assert_eq!(expand_home("~other"), "~other");
        assert_eq!(expand_home("a~/b"), "a~/b");
    }

    #[test]
    fn config_load_save_import() {
        let dir = temp_dir("config");
        let path = dir.join("mcp.json");
        assert!(load_in(&path).unwrap().is_empty());
        fs::write(
            &path,
            r#"{"other": 1, "mcpServers": {"fs": {"command": "npx", "args": ["-y", "x"], "env": {"K": "v"}, "type": "stdio"}}}"#,
        )
        .unwrap();
        let mut list = load_in(&path).unwrap();
        assert_eq!(list[0].name, "fs");
        assert_eq!(list[0].env, vec![("K".to_string(), "v".to_string())]);
        assert!(!list[0].disabled);
        list[0].disabled = true;
        save_in(&path, &list).unwrap();
        let root: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(root["other"], 1);
        assert_eq!(root["mcpServers"]["fs"]["type"], "stdio");
        assert_eq!(root["mcpServers"]["fs"]["disabled"], true);
        assert_eq!(load_in(&path).unwrap(), list);
        let claude = dir.join("claude.json");
        fs::write(
            &claude,
            r#"{"mcpServers": {"fs": {"command": "x"}, "git": {"command": "uvx", "args": ["mcp-server-git"]}, "web": {"url": "https://x"}}}"#,
        )
        .unwrap();
        let result = import_in(&path, &claude).unwrap();
        assert_eq!((result.added, result.existing, result.skipped), (1, 1, 1));
        let list = load_in(&path).unwrap();
        let git = list.iter().find(|s| s.name == "git").unwrap();
        assert!(git.disabled);
        assert_eq!(git.args, vec!["mcp-server-git"]);
        assert!(import_in(&path, &dir.join("missing.json")).is_err());
        fs::write(&path, "{broken").unwrap();
        assert!(load_in(&path).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
