use crate::i18n::{tr, trf};
use crate::sandbox::Folder;
use crate::shell;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_OUTPUT: usize = 8000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct CustomTool {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub params: Vec<String>,
    pub command: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dream-llmchatbox")
}

pub fn tools_dir() -> PathBuf {
    config_dir().join("tools")
}

fn migrate_old_file(dir: &Path) {
    let old = config_dir().join("tools.json");
    let Ok(text) = fs::read_to_string(&old) else {
        return;
    };
    let list: Vec<CustomTool> = serde_json::from_str(&text).unwrap_or_default();
    if write_all(dir, &list).is_ok() {
        let _ = fs::remove_file(old);
    }
}

pub fn load() -> Vec<CustomTool> {
    let dir = tools_dir();
    migrate_old_file(&dir);
    load_from(&dir)
}

fn load_from(dir: &Path) -> Vec<CustomTool> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut list: Vec<CustomTool> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| {
            let mut tool: CustomTool = serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?;
            if tool.name.is_empty() {
                tool.name = path.file_stem()?.to_string_lossy().to_string();
            }
            Some(tool)
        })
        .collect();
    list.sort_by(|a, b| a.name.cmp(&b.name));
    list
}

pub fn save(tools: &[CustomTool]) -> Result<(), String> {
    save_to(&tools_dir(), tools)
}

fn write_all(dir: &Path, tools: &[CustomTool]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for tool in tools {
        let json = serde_json::to_string_pretty(tool).map_err(|e| e.to_string())?;
        fs::write(dir.join(format!("{}.json", tool.name)), json).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn save_to(dir: &Path, tools: &[CustomTool]) -> Result<(), String> {
    write_all(dir, tools)?;
    if let Ok(entries) = fs::read_dir(dir) {
        for path in entries.flatten().map(|entry| entry.path()) {
            let stem = path.file_stem().map(|s| s.to_string_lossy().to_string());
            let is_json = path.extension().is_some_and(|ext| ext == "json");
            if is_json && stem.is_some_and(|stem| !tools.iter().any(|t| t.name == stem)) {
                let _ = fs::remove_file(path);
            }
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct ExportFile {
    dream_llmchatbox_tools: u32,
    tools: Vec<CustomTool>,
}

pub fn export_json(tools: &[CustomTool]) -> String {
    serde_json::to_string_pretty(&ExportFile {
        dream_llmchatbox_tools: 1,
        tools: tools.to_vec(),
    })
    .unwrap_or_default()
}

pub fn import_json(text: &str, list: &mut Vec<CustomTool>, taken: &[String]) -> Result<usize, String> {
    let incoming: Vec<CustomTool> = serde_json::from_str::<ExportFile>(text)
        .map(|file| file.tools)
        .or_else(|_| serde_json::from_str::<Vec<CustomTool>>(text))
        .or_else(|_| serde_json::from_str::<CustomTool>(text).map(|tool| vec![tool]))
        .map_err(|_| tr("This is not a Dream-LLMChatBox tools file"))?;
    let mut added = 0;
    for mut tool in incoming {
        let same = list.iter().any(|t| {
            t.name == tool.name && t.command == tool.command && t.params == tool.params
        });
        if same {
            continue;
        }
        let base = tool.name.clone();
        let mut number = 2;
        let used = |name: &str, list: &[CustomTool]| {
            taken.iter().any(|t| t == name) || list.iter().any(|t| t.name == name)
        };
        while used(&tool.name, list) {
            tool.name = format!("{base}_{number}");
            number += 1;
        }
        tool.enabled = false;
        let names: Vec<String> = taken
            .iter()
            .cloned()
            .chain(list.iter().map(|t| t.name.clone()))
            .collect();
        validate(&tool, &names).map_err(|e| format!("{}: {e}", tool.name))?;
        list.push(tool);
        added += 1;
    }
    Ok(added)
}

pub fn parse_params(text: &str) -> Vec<String> {
    text.split([',', ' '])
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub fn validate(tool: &CustomTool, taken: &[String]) -> Result<(), String> {
    if taken.contains(&tool.name) {
        return Err(trf("A tool named {} already exists", &[&tool.name]));
    }
    validate_fields(tool, false)
}

pub fn validate_fields(tool: &CustomTool, command_optional: bool) -> Result<(), String> {
    if !valid_name(&tool.name) {
        return Err(tr("Name: only a-z, 0-9 and _, starting with a letter"));
    }
    if tool.description.trim().is_empty() {
        return Err(tr("Description: tell the model what the tool does and when to use it"));
    }
    if let Some(bad) = tool.params.iter().find(|param| !valid_name(param)) {
        return Err(trf("Parameter {}: only a-z, 0-9 and _", &[&bad]));
    }
    if !command_optional && tool.command.trim().is_empty() {
        return Err(tr("Command is empty"));
    }
    Ok(())
}

pub fn definition(tool: &CustomTool) -> Value {
    let properties: serde_json::Map<String, Value> = tool
        .params
        .iter()
        .map(|param| (param.clone(), json!({ "type": "string" })))
        .collect();
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": tool.params,
            }
        }
    })
}

pub fn run(tool: &CustomTool, arguments: &Value, folders: &[Folder]) -> String {
    let directory = folders
        .first()
        .map(|folder| PathBuf::from(&folder.path))
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut env = vec![("TOOLS_DIR".to_string(), tools_dir().to_string_lossy().to_string())];
    for param in &tool.params {
        let value = match &arguments[param] {
            Value::String(text) => text.clone(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        env.push((param.clone(), value));
    }
    match shell::run(&tool.command, &directory, &env, TIMEOUT, None) {
        Ok(output) => {
            let mut text = output.text.clone();
            if text.chars().count() > MAX_OUTPUT {
                text = text.chars().take(MAX_OUTPUT).collect::<String>() + "\n[output cut]";
            }
            output.summary(&text)
        }
        Err(e) => format!("Error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(command: &str, params: &[&str]) -> CustomTool {
        CustomTool {
            name: "test_tool".to_string(),
            description: "Test".to_string(),
            params: params.iter().map(|p| p.to_string()).collect(),
            command: command.to_string(),
            enabled: true,
        }
    }

    #[test]
    fn runs_with_parameters_as_variables() {
        let echo = tool("printf '%s' \"$text\"", &["text"]);
        assert_eq!(run(&echo, &json!({"text": "a; rm -rf x"}), &[]), "a; rm -rf x");
        let failing = tool("echo oops >&2; exit 3", &[]);
        assert_eq!(run(&failing, &json!({}), &[]), "oops\n[exit code 3]");
        assert_eq!(run(&tool("true", &[]), &json!({}), &[]), "(no output)");
    }

    #[test]
    fn export_and_import() {
        let mine = vec![tool("df -h", &[])];
        let text = export_json(&mine);
        let mut list = mine.clone();
        assert_eq!(import_json(&text, &mut list, &[]).unwrap(), 0);

        let mut other = tool("du -sh .", &[]);
        other.name = "test_tool".to_string();
        let text = export_json(&[other]);
        assert_eq!(import_json(&text, &mut list, &[]).unwrap(), 1);
        assert_eq!(list[1].name, "test_tool_2");
        assert!(!list[1].enabled);
        assert!(import_json("{}", &mut list, &[]).is_err());
    }

    #[test]
    fn one_file_per_tool() {
        let dir = std::env::temp_dir().join(format!("dlc-tools-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut a = tool("df -h", &[]);
        a.name = "disk".into();
        let b = tool("ls", &[]);
        save_to(&dir, &[a.clone(), b.clone()]).unwrap();
        fs::write(dir.join("helper.py"), "print(1)").unwrap();
        assert!(dir.join("disk.json").exists());
        assert_eq!(load_from(&dir), vec![a.clone(), b]);
        save_to(&dir, &[a.clone()]).unwrap();
        assert!(!dir.join("test_tool.json").exists());
        assert!(dir.join("helper.py").exists());
        assert_eq!(load_from(&dir), vec![a]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn checks_input() {
        let taken = vec!["calculate".to_string()];
        assert!(validate(&tool("ls", &["path"]), &taken).is_ok());
        assert!(validate(&tool("ls", &["Bad-Name"]), &taken).is_err());
        let mut clash = tool("ls", &[]);
        clash.name = "calculate".to_string();
        assert!(validate(&clash, &taken).is_err());
        assert_eq!(parse_params("query, limit  path"), vec!["query", "limit", "path"]);
        let definition = definition(&tool("ls", &["path"]));
        assert_eq!(definition["function"]["parameters"]["required"][0], "path");
    }
}
