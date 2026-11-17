use crate::i18n::{tr, trf};
use crate::custom_tools::{self, CustomTool};
use crate::commands;
pub use crate::commands::Context;
use crate::file_tools;
use crate::hardware;
use crate::mcp;
use crate::ollama::ToolCall;
use crate::sandbox::Folder;
use crate::settings;
use crate::memory_tools;
use crate::sandbox;
use crate::start_args;
use crate::web_tools;
use serde_json::{Value, json};

pub const MAX_STEPS: usize = 10;

pub const BUILTIN: [(&str, &str); 22] = [
    ("get_current_time", "Current date, time and weekday of this computer"),
    ("calculate", "Exact math, e.g. (12.5 * 4) / 3 or sqrt(2)"),
    ("get_hardware_info", "Graphics card, VRAM and RAM of this computer"),
    ("remember", "The model saves lasting facts or wishes to \"Start arguments\""),
    ("ask_user", "The model asks you a question with up to 4 answers to click, or your own answer"),
    ("pause_and_wait", "The model ends its answer and continues later (Continue button or automatically)"),
    ("list_projects", "Lists your projects (group folders + global memory) so the model knows where things are"),
    ("request_project_access", "The model asks you for access to one of your projects (read / read + write / no)"),
    ("list_folder", "Lists files in shared folders"),
    ("read_file", "Reads text files in shared folders"),
    ("search_files", "Searches text in all files of shared folders"),
    ("write_file", "Writes files in shared folders set to \"read + write\""),
    ("edit_file", "Changes only a part of a file (asks like write_file)"),
    ("create_document", "Creates PDF, Word (DOCX), ODT, slides (PPTX), HTML or EPUB from Markdown – asks like write_file, needs pandoc"),
    ("memory_list", "Lists the notes in the group memory (only in chats of a group)"),
    ("memory_search", "Searches the group memory"),
    ("memory_read", "Reads a note from the group memory"),
    ("memory_write", "Writes notes into the group memory – every change shows up in the chat"),
    ("web_search", "Searches the web with DuckDuckGo – asks you first"),
    ("open_web_page", "Opens a web page and reads it as clean text – asks you first"),
    ("sync_from_project", "Copies files from your original project into the test environment (only there, without asking)"),
    ("run_terminal_command", "Runs commands in the first shared folder – asks you first (safe commands run directly)"),
];

pub fn definitions(folders: &[Folder], has_group: bool) -> Value {
    let mut items = definitions_with(folders, has_group, &settings::load().disabled_tools, &custom_tools::load());
    if let Value::Array(list) = &mut items {
        list.extend(mcp::definitions());
    }
    items
}

pub fn is_builtin(name: &str) -> bool {
    BUILTIN.iter().any(|(builtin, _)| *builtin == name)
}

fn default_definition(name: &str) -> Option<Value> {
    let every_folder = [Folder {
        path: "/".to_string(),
        writable: true,
    }];
    builtin_definitions(&every_folder, true)
        .into_iter()
        .find(|item| item["function"]["name"] == name)
}

pub fn default_params(name: &str) -> Vec<String> {
    default_definition(name)
        .and_then(|item| {
            item["function"]["parameters"]["properties"]
                .as_object()
                .map(|map| map.keys().cloned().collect())
        })
        .unwrap_or_default()
}

pub fn default_description(name: &str) -> String {
    default_definition(name)
        .and_then(|item| item["function"]["description"].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn builtin_definitions(folders: &[Folder], has_group: bool) -> Vec<Value> {
    let mut items = match basic_definitions() {
        Value::Array(items) => items,
        _ => Vec::new(),
    };
    items.extend(file_tools::definitions(folders));
    if folders.iter().any(|folder| folder.writable) {
        items.push(crate::documents::definition());
    }
    if !folders.is_empty() {
        items.push(commands::definition());
    }
    if crate::project_sync::available(folders) {
        items.push(crate::project_sync::definition());
    }
    items.extend(web_tools::definitions());
    if has_group {
        items.extend(memory_tools::definitions());
    }
    items
}

fn definitions_with(
    folders: &[Folder],
    has_group: bool,
    disabled: &[String],
    files: &[CustomTool],
) -> Value {
    let mut items = builtin_definitions(folders, has_group);
    items.retain(|item| {
        let name = item["function"]["name"].as_str().unwrap_or_default();
        !disabled.iter().any(|off| off == name)
    });
    for item in &mut items {
        let name = item["function"]["name"].as_str().unwrap_or_default().to_string();
        let Some(file) = files.iter().find(|file| file.name == name) else {
            continue;
        };
        if !file.command.trim().is_empty() {
            *item = custom_tools::definition(file);
        } else if !file.description.trim().is_empty() {
            item["function"]["description"] = Value::String(file.description.clone());
        }
    }
    items.extend(
        files
            .iter()
            .filter(|tool| tool.enabled && !is_builtin(&tool.name))
            .map(custom_tools::definition),
    );
    Value::Array(items)
}

fn basic_definitions() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "get_current_time",
                "description": "Get the current local date, time and weekday of the user's computer.",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "calculate",
                "description": "Calculate a math expression exactly. Supports + - * / ^ %, parentheses, decimals and the functions sqrt, abs, round, floor, ceil, sin, cos, tan, ln, log10 and the constants pi and e.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "expression": {
                            "type": "string",
                            "description": "The expression, for example (12.5 * 4) / 3 or sqrt(2)"
                        }
                    },
                    "required": ["expression"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_hardware_info",
                "description": "Get the user's graphics card, VRAM and system RAM. Use it for questions about which AI models fit on this computer.",
                "parameters": { "type": "object", "properties": {}, "required": [] }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "remember",
                "description": "Save a short, lasting fact or wish of the user (for example their setup or how they like answers) so it is known in every future chat. Only use it when the user asks you to remember something or tells you a lasting preference.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "One short sentence, for example: Uses Arch Linux with KDE" }
                    },
                    "required": ["text"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "ask_user",
                "description": "Ask the user a question when you need a decision or information you don't have, instead of guessing. Give 2 to 4 short answer options the user can click; the user can also type an own answer. Wait for the answer before you continue.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "question": { "type": "string", "description": "The question, short and clear" },
                        "options": { "type": "array", "items": { "type": "string" }, "description": "2 to 4 short answer options (max. 4)" }
                    },
                    "required": ["question"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "pause_and_wait",
                "description": "End your answer now and continue later, only when you must wait for something you can't wait for yourself (a download, a service starting, something the user does). Never use it for builds or tests: run those with run_terminal_command and wait for the result. The app shows a Continue button and can continue automatically after the given minutes.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "reason": { "type": "string", "description": "What you are waiting for, short" },
                        "minutes": { "type": "integer", "description": "How long to wait, 1 to 30. Default 5." }
                    },
                    "required": ["reason"]
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_projects",
                "description": "List the user's known projects (global memory): name, folder, short note and whether it has an ARCHITEKTUR.md. Use it when the user talks about another project or you need to know where something is. Knowing a folder does not give you access.",
                "parameters": { "type": "object", "properties": {} }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "request_project_access",
                "description": "Ask the user for access to one project from list_projects. The user decides: read only, read + write, or no. Only ask when you really need files from that project.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "project": { "type": "string", "description": "Project name as shown by list_projects" },
                        "reason": { "type": "string", "description": "Why you need it, one short sentence" }
                    },
                    "required": ["project", "reason"]
                }
            }
        }
    ])
}

pub fn activity(call: &ToolCall) -> (&'static str, String) {
    let args = &call.function.arguments;
    let text = |key: &str| {
        let value = args[key].as_str().unwrap_or_default();
        let short: String = value.chars().take(70).collect();
        if value.chars().count() > 70 { format!("{short}…") } else { short }
    };
    match call.function.name.as_str() {
        "web_search" => ("🔍", trf("Searching: {}", &[&text("query")])),
        "open_web_page" => ("🌐", trf("Opening {}", &[&text("url")])),
        "run_terminal_command" => ("$", trf("Running: {}", &[&text("command")])),
        "read_file" => ("📄", trf("Reading {}", &[&text("path")])),
        "list_folder" => ("📁", trf("Looking into {}", &[&text("path")])),
        "write_file" => ("✏", trf("Writing {}", &[&text("path")])),
        "edit_file" => ("✏", trf("Editing {}", &[&text("path")])),
        "create_document" => ("📝", trf("Creating document {}", &[&text("path")])),
        "search_files" => ("🔎", trf("Searching files for {}", &[&text("query")])),
        "calculate" => ("=", trf("Calculating {}", &[&text("expression")])),
        "get_current_time" => ("🕒", tr("Checking the time")),
        "get_hardware_info" => ("🖥", tr("Checking the hardware")),
        "remember" => ("📌", trf("Remembering: {}", &[&text("text")])),
        "ask_user" => ("❓", trf("Asking you: {}", &[&text("question")])),
        "pause_and_wait" => ("⏸", trf("Pausing: {}", &[&text("reason")])),
        "list_projects" => ("🗂", tr("Looking up your projects")),
        "request_project_access" => ("🔑", trf("Asking for access to {}", &[&text("project")])),
        "memory_list" => ("🧠", tr("Looking into the group memory")),
        "memory_search" => ("🧠", trf("Searching the memory: {}", &[&text("query")])),
        "memory_read" => ("🧠", trf("Reading memory: {}", &[&text("name")])),
        "memory_write" => ("🧠", trf("Writing memory: {}", &[&text("name")])),
        name if name.starts_with(mcp::PREFIX) => ("🔌", mcp::label(name)),
        _ => ("⚙", describe_call(call)),
    }
}

pub fn describe_call(call: &ToolCall) -> String {
    let arguments = match &call.function.arguments {
        Value::Object(map) if !map.is_empty() => map
            .iter()
            .map(|(key, value)| match value {
                Value::String(text) if text.chars().count() > 80 || text.contains('\n') => {
                    format!("{key}: <{} lines>", text.lines().count())
                }
                Value::String(text) => format!("{key}: {text}"),
                other => format!("{key}: {other}"),
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    };
    format!("{}({arguments})", call.function.name)
}

pub fn run(call: &ToolCall, context: &Context) -> String {
    let folders = context.folders;
    let name = call.function.name.as_str();
    if settings::load().disabled_tools.iter().any(|off| off == name) {
        return format!("Error: the user turned the tool '{name}' off");
    }
    let file = custom_tools::load().into_iter().find(|tool| tool.name == name);
    match file {
        Some(tool) if !tool.command.trim().is_empty() && (tool.enabled || is_builtin(name)) => {
            custom_tools::run(&tool, &call.function.arguments, folders)
        }
        Some(tool) if !is_builtin(name) && !tool.enabled => {
            format!("Error: the user turned the tool '{name}' off")
        }
        _ => run_builtin(call, context),
    }
}

fn approve_write(arguments: &Value, context: &Context) -> Result<(), String> {
    if commands::allowed_in_chat(context.chat_id, "write_file") {
        return Ok(());
    }
    let path = arguments["path"].as_str().unwrap_or_default();
    let Ok(target) = sandbox::resolve(context.folders, path) else {
        return Ok(());
    };
    if !target.writable || crate::workspace::is_inside(&target.path.to_string_lossy()) {
        return Ok(());
    }
    let editing = arguments.get("old_text").is_some();
    let lines = arguments["content"]
        .as_str()
        .or_else(|| arguments["new_text"].as_str())
        .map_or(0, |content| content.lines().count());
    let title = if editing {
        tr("The model wants to change a part of a file:")
    } else if target.path.exists() {
        tr("The model wants to overwrite a file:")
    } else {
        tr("The model wants to create a file:")
    };
    let note = trf("{} lines  ·  \"Allow in this chat\" = all file writes in this chat", &[&lines]);
    let mut text = target.path.display().to_string();
    if editing {
        let preview = |key: &str, sign: &str| {
            arguments[key]
                .as_str()
                .unwrap_or_default()
                .lines()
                .take(6)
                .map(|line| format!("{sign} {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        text = format!("{text}\n{}\n{}", preview("old_text", "−"), preview("new_text", "+"));
    }
    match (context.ask_web)(&title, &text, &note) {
        commands::Approval::Once => Ok(()),
        commands::Approval::Chat => {
            commands::allow_in_chat(context.chat_id, "write_file");
            Ok(())
        }
        commands::Approval::No => Err("The user did not allow writing this file.".to_string()),
    }
}

fn run_builtin(call: &ToolCall, context: &Context) -> String {
    let folders = context.folders;
    if call.function.name == "pause_and_wait" || call.function.name == "request_project_access" {
        return "Error: this only works in the chat window".to_string();
    }
    if call.function.name == "list_projects" {
        return crate::projects::listing();
    }
    if call.function.name == "ask_user" {
        return "Error: questions can only be asked in the chat window".to_string();
    }
    let writes = ["write_file", "edit_file", crate::documents::NAME];
    if writes.contains(&call.function.name.as_str()) {
        if let Err(refused) = approve_write(&call.function.arguments, context) {
            return refused;
        }
        if let Some(dir) = context.checkpoint {
            crate::checkpoint::save(dir, folders, call.function.arguments["path"].as_str().unwrap_or_default());
        }
    }
    if call.function.name == crate::documents::NAME {
        return crate::documents::run(&call.function.arguments, folders);
    }
    if let Some(output) = file_tools::run(&call.function.name, &call.function.arguments, folders) {
        return output;
    }
    if call.function.name == commands::NAME {
        return commands::run(&call.function.arguments, context);
    }
    if let Some(output) = mcp::run(&call.function.name, &call.function.arguments, context) {
        return output;
    }
    if let Some(output) = web_tools::run(&call.function.name, &call.function.arguments, context) {
        return output;
    }
    if let Some(output) = memory_tools::run(&call.function.name, &call.function.arguments, context.group) {
        return output;
    }
    if let Some(output) = crate::project_sync::run(&call.function.name, &call.function.arguments, context.folders) {
        return output;
    }
    match call.function.name.as_str() {
        "get_current_time" => {
            let now = chrono::Local::now();
            now.format("%Y-%m-%d %H:%M:%S, %A (UTC%:z)").to_string()
        }
        "calculate" => {
            let expression = call.function.arguments["expression"]
                .as_str()
                .unwrap_or_default();
            match evaluate(expression) {
                Ok(value) => format_number(value),
                Err(e) => format!("Error: {e}"),
            }
        }
        "get_hardware_info" => hardware::get().summary(),
        "remember" => start_args::remember(call.function.arguments["text"].as_str().unwrap_or_default()),
        other => format!("Error: unknown tool '{other}'"),
    }
}

fn format_number(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        let text = format!("{value:.10}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

pub fn evaluate(expression: &str) -> Result<f64, String> {
    let tokens = tokenize(expression)?;
    let mut parser = Parser { tokens, position: 0 };
    let value = parser.expression()?;
    if parser.position < parser.tokens.len() {
        return Err("unexpected input at the end".to_string());
    }
    if !value.is_finite() {
        return Err("result is not a finite number".to_string());
    }
    Ok(value)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    Name(String),
    Operator(char),
    Open,
    Close,
}

fn tokenize(text: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' || c == ',' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == ',') {
                i += 1;
            }
            let number: String = chars[start..i].iter().collect::<String>().replace(',', ".");
            tokens.push(Token::Number(
                number.parse().map_err(|_| format!("invalid number '{number}'"))?,
            ));
        } else if c.is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                i += 1;
            }
            tokens.push(Token::Name(chars[start..i].iter().collect::<String>().to_lowercase()));
        } else if "+-*/^%×÷".contains(c) {
            let operator = match c {
                '×' => '*',
                '÷' => '/',
                other => other,
            };
            tokens.push(Token::Operator(operator));
            i += 1;
        } else if c == '(' {
            tokens.push(Token::Open);
            i += 1;
        } else if c == ')' {
            tokens.push(Token::Close);
            i += 1;
        } else {
            return Err(format!("unexpected character '{c}'"));
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        self.position += 1;
        token
    }

    fn expression(&mut self) -> Result<f64, String> {
        let mut value = self.term()?;
        while let Some(Token::Operator(op @ ('+' | '-'))) = self.peek().cloned() {
            self.position += 1;
            let right = self.term()?;
            value = if op == '+' { value + right } else { value - right };
        }
        Ok(value)
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut value = self.power()?;
        while let Some(Token::Operator(op @ ('*' | '/' | '%'))) = self.peek().cloned() {
            self.position += 1;
            let right = self.power()?;
            value = match op {
                '*' => value * right,
                '/' if right == 0.0 => return Err("division by zero".to_string()),
                '/' => value / right,
                _ => value % right,
            };
        }
        Ok(value)
    }

    fn power(&mut self) -> Result<f64, String> {
        let base = self.unary()?;
        if let Some(Token::Operator('^')) = self.peek() {
            self.position += 1;
            let exponent = self.power()?;
            return Ok(base.powf(exponent));
        }
        Ok(base)
    }

    fn unary(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some(Token::Operator('-')) => {
                self.position += 1;
                Ok(-self.unary()?)
            }
            Some(Token::Operator('+')) => {
                self.position += 1;
                self.unary()
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<f64, String> {
        match self.next() {
            Some(Token::Number(value)) => Ok(value),
            Some(Token::Open) => {
                let value = self.expression()?;
                match self.next() {
                    Some(Token::Close) => Ok(value),
                    _ => Err("missing ')'".to_string()),
                }
            }
            Some(Token::Name(name)) => match name.as_str() {
                "pi" => Ok(std::f64::consts::PI),
                "e" => Ok(std::f64::consts::E),
                function => {
                    let argument = match self.next() {
                        Some(Token::Open) => {
                            let value = self.expression()?;
                            match self.next() {
                                Some(Token::Close) => value,
                                _ => return Err("missing ')'".to_string()),
                            }
                        }
                        _ => return Err(format!("'{function}' needs parentheses")),
                    };
                    match function {
                        "sqrt" if argument < 0.0 => Err("sqrt of a negative number".to_string()),
                        "sqrt" => Ok(argument.sqrt()),
                        "abs" => Ok(argument.abs()),
                        "round" => Ok(argument.round()),
                        "floor" => Ok(argument.floor()),
                        "ceil" => Ok(argument.ceil()),
                        "sin" => Ok(argument.sin()),
                        "cos" => Ok(argument.cos()),
                        "tan" => Ok(argument.tan()),
                        "ln" => Ok(argument.ln()),
                        "log10" | "log" => Ok(argument.log10()),
                        other => Err(format!("unknown function '{other}'")),
                    }
                }
            },
            Some(token) => Err(format!("unexpected {token:?}")),
            None => Err("expression ends too early".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama::ToolFunction;

    fn call(name: &str, arguments: Value) -> ToolCall {
        ToolCall {
            function: ToolFunction {
                name: name.to_string(),
                arguments,
            },
        }
    }

    #[test]
    fn calculator() {
        assert_eq!(evaluate("2 + 3 * 4").unwrap(), 14.0);
        assert_eq!(evaluate("(2 + 3) * 4").unwrap(), 20.0);
        assert_eq!(evaluate("2^3^2").unwrap(), 512.0);
        assert_eq!(evaluate("-2^2").unwrap(), 4.0);
        assert_eq!(evaluate("10 % 4").unwrap(), 2.0);
        assert_eq!(evaluate("sqrt(16) + abs(-3)").unwrap(), 7.0);
        assert_eq!(evaluate("3,5 × 2").unwrap(), 7.0);
        assert!((evaluate("sin(pi/2)").unwrap() - 1.0).abs() < 1e-12);
        assert!(evaluate("1/0").is_err());
        assert!(evaluate("2 +").is_err());
        assert!(evaluate("rm -rf").is_err());
    }

    fn run_basic(call: &ToolCall) -> String {
        let no = |_: &str, _: &std::path::Path, _: bool| crate::commands::Approval::No;
        let context = Context {
            folders: &[],
            chat_id: "",
            safe: &[],
            cancel: None,
            group: "",
            ask: &no,
            ask_web: &|_: &str, _: &str, _: &str| crate::commands::Approval::No,
            checkpoint: None,
        };
        run_builtin(call, &context)
    }

    #[test]
    fn turn_off_and_add() {
        let names = |value: Value| -> Vec<String> {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|item| item["function"]["name"].as_str().unwrap().to_string())
                .collect()
        };
        let own = CustomTool {
            name: "disk_usage".into(),
            description: "Free disk space".into(),
            params: Vec::new(),
            command: "df -h".into(),
            enabled: true,
        };
        let remember = CustomTool {
            name: "remember".into(),
            description: "Only when asked".into(),
            ..Default::default()
        };
        let time = CustomTool {
            name: "get_current_time".into(),
            description: "Time from bash".into(),
            params: vec!["zone".into()],
            command: "date".into(),
            enabled: true,
        };
        let all = definitions_with(&[], false, &["calculate".to_string()], &[own, remember, time]);
        assert_eq!(all[2]["function"]["description"], "Only when asked");
        assert_eq!(all[0]["function"]["parameters"]["required"][0], "zone");
        assert_eq!(default_params("read_file"), vec!["path", "start_line"]);
        assert!(default_description("write_file").starts_with("Create or overwrite"));
        let list = names(all);
        assert_eq!(list, vec!["get_current_time", "get_hardware_info", "remember", "ask_user", "pause_and_wait", "list_projects", "request_project_access", "web_search", "open_web_page", "disk_usage"]);
    }

    #[test]
    fn long_arguments_are_short() {
        let text = describe_call(&call("write_file", json!({"path": "a.rs", "content": "a\nb\nc"})));
        assert_eq!(text, "write_file(content: <3 lines>, path: a.rs)");
    }

    #[test]
    fn write_needs_approval() {
        let dir = std::env::temp_dir().join(format!("dlc-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let folders = vec![Folder {
            path: dir.canonicalize().unwrap().to_string_lossy().to_string(),
            writable: true,
        }];
        let no = |_: &str, _: &std::path::Path, _: bool| crate::commands::Approval::No;
        let refuse = |_: &str, _: &str, _: &str| crate::commands::Approval::No;
        let context = Context {
            folders: &folders,
            chat_id: "write-test",
            safe: &[],
            cancel: None,
            group: "",
            ask: &no,
            ask_web: &refuse,
            checkpoint: None,
        };
        let write = call("write_file", json!({"path": "a.md", "content": "x"}));
        assert!(run_builtin(&write, &context).contains("did not allow"));
        assert!(!dir.join("a.md").exists());
        let allow = |_: &str, _: &str, _: &str| crate::commands::Approval::Once;
        let context = Context { ask_web: &allow, ..context };
        assert!(run_builtin(&write, &context).starts_with("Created"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_tools() {
        assert_eq!(run_basic(&call("calculate", json!({"expression": "1/3"}))), "0.3333333333");
        assert_eq!(run_basic(&call("calculate", json!({"expression": "6*7"}))), "42");
        assert!(run_basic(&call("get_current_time", json!({}))).contains("UTC"));
        assert!(run_basic(&call("unknown", json!({}))).starts_with("Error"));
        assert_eq!(
            describe_call(&call("calculate", json!({"expression": "6*7"}))),
            "calculate(expression: 6*7)"
        );
        assert_eq!(definitions_with(&[], false, &[], &[]).as_array().unwrap().len(), 10);
        assert_eq!(definitions_with(&[], true, &[], &[]).as_array().unwrap().len(), 14);
    }
}
