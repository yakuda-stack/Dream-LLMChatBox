use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const MAX_ENTRIES: usize = 100;
const MAX_CHARS: usize = 500;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct StartArg {
    pub text: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub by_model: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub standard: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub choice: String,
}

pub const STANDARD: [(&str, &str); 6] = [
    (
        "code_blocks",
        "Put only real code, terminal commands and config files into fenced code blocks: three backticks and the language name (for example ```rust or ```bash) on the line before, three backticks on the line after. Write all normal text, explanations and lists as plain text outside of code blocks. Never wrap your whole answer in a code block and never use ```markdown or ```text.",
    ),
    (
        "use_tools",
        "If a tool can give an exact answer (time, math, files, hardware), use it instead of guessing.",
    ),
    (
        "honest",
        "If you are not sure about something, say so instead of making something up.",
    ),
    (
        "ask_when_unknown",
        "If you do not know the answer or your knowledge may be outdated, never invent facts, names, numbers, versions or links. Stop and ask the user first: may you search the web for it, or does the user already know the answer? If the ask_user tool is available, use it with options like \"Search the web\" and \"I'll tell you\". Only continue after the user answered.",
    ),
    (
        "no_code_comments",
        "Never write comments inside code blocks. Explain the code in one or two short sentences above the code block instead.",
    ),
    (
        crate::answer_style::ID,
        "Answer in the style of a well-known online AI. Choose the style next to this entry; every style is a Markdown file in the answer_styles folder that you can edit or copy.",
    ),
];

pub const OFF_BY_DEFAULT: [&str; 2] = ["no_code_comments", crate::answer_style::ID];

const TITLES: [(&str, &str); 6] = [
    ("code_blocks", "Code only in code blocks"),
    ("use_tools", "Use tools instead of guessing"),
    ("honest", "Say when you are not sure"),
    ("ask_when_unknown", "Ask before guessing (web or user)"),
    ("no_code_comments", "No comments in code"),
    (crate::answer_style::ID, "Answer like an online AI"),
];

const TITLE_CHARS: usize = 48;

pub fn title(arg: &StartArg) -> String {
    if let Some((_, title)) = TITLES.iter().find(|(id, _)| *id == arg.standard) {
        return title.to_string();
    }
    let text = arg.text.trim();
    let first = text
        .split_inclusive(['.', '!', '?', '\n'])
        .next()
        .unwrap_or(text)
        .trim()
        .trim_end_matches(['.', '!', '?']);
    if first.chars().count() <= TITLE_CHARS {
        return first.to_string();
    }
    let mut short = String::new();
    for word in first.split_whitespace() {
        if short.chars().count() + word.chars().count() + 1 > TITLE_CHARS {
            break;
        }
        if !short.is_empty() {
            short.push(' ');
        }
        short.push_str(word);
    }
    format!("{short}…")
}

pub fn key(arg: &StartArg) -> String {
    if arg.standard.is_empty() {
        arg.text.trim().to_string()
    } else {
        arg.standard.clone()
    }
}

pub fn for_group(list: &[StartArg], extra: &[String], removed: &[String], use_global: bool) -> Vec<StartArg> {
    list.iter()
        .map(|arg| {
            let key = key(arg);
            StartArg {
                enabled: (use_global && arg.enabled && !removed.contains(&key)) || extra.contains(&key),
                ..arg.clone()
            }
        })
        .collect()
}

pub fn standard_text(id: &str) -> Option<&'static str> {
    STANDARD
        .iter()
        .find(|(standard, _)| *standard == id)
        .map(|(_, text)| *text)
}

fn with_standard(mut list: Vec<StartArg>) -> Vec<StartArg> {
    for (position, (id, text)) in STANDARD.iter().enumerate() {
        if !list.iter().any(|arg| arg.standard == *id) {
            let at = position.min(list.len());
            list.insert(
                at,
                StartArg {
                    text: text.to_string(),
                    enabled: !OFF_BY_DEFAULT.contains(id),
                    by_model: false,
                    standard: id.to_string(),
                    choice: if *id == crate::answer_style::ID {
                        crate::answer_style::DEFAULT_CHOICE.to_string()
                    } else {
                        String::new()
                    },
                },
            );
        }
    }
    list
}

fn default_true() -> bool {
    true
}

fn path() -> PathBuf {
    crate::storage::data_dir().join("start_args.json")
}

pub fn load() -> Vec<StartArg> {
    with_standard(
        fs::read_to_string(path())
            .ok()
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default(),
    )
}

pub fn save(list: &[StartArg]) -> Result<(), String> {
    let path = path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

pub fn system_note(list: &[StartArg]) -> Option<String> {
    let lines: Vec<String> = list
        .iter()
        .filter(|arg| arg.enabled && !arg.text.trim().is_empty())
        .filter_map(|arg| {
            if arg.standard == crate::answer_style::ID {
                crate::answer_style::instruction(&arg.choice).map(|text| format!("- {text}"))
            } else {
                Some(format!("- {}", arg.text.trim()))
            }
        })
        .collect();
    (!lines.is_empty()).then(|| {
        format!(
            "Always follow these points:\n{}",
            lines.join("\n")
        )
    })
}

pub fn add(list: &mut Vec<StartArg>, text: &str, by_model: bool) -> Result<String, String> {
    let text: String = text.trim().chars().take(MAX_CHARS).collect();
    if text.is_empty() {
        return Err("Nothing to remember".to_string());
    }
    if list
        .iter()
        .any(|arg| arg.text.trim().eq_ignore_ascii_case(&text))
    {
        return Ok(format!("Already saved: {text}"));
    }
    if list.len() >= MAX_ENTRIES {
        return Err(format!("There are already {MAX_ENTRIES} start arguments"));
    }
    list.push(StartArg {
        text: text.clone(),
        enabled: true,
        by_model,
        standard: String::new(),
        choice: String::new(),
    });
    Ok(format!("Saved to start arguments: {text}"))
}

pub fn remember(text: &str) -> String {
    let mut list = load();
    match add(&mut list, text, true) {
        Ok(message) => match save(&list) {
            Ok(()) => message,
            Err(e) => format!("Error: {e}"),
        },
        Err(e) => format!("Error: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_entries() {
        let list = with_standard(Vec::new());
        assert_eq!(list.len(), STANDARD.len());
        assert!(list.iter().all(|arg| arg.enabled != OFF_BY_DEFAULT.contains(&arg.standard.as_str())));
        let mut changed = list.clone();
        changed[0].enabled = false;
        changed.push(StartArg {
            text: "Own".into(),
            enabled: true,
            ..Default::default()
        });
        assert_eq!(with_standard(changed.clone()), changed);
        assert_eq!(standard_text("honest"), Some(STANDARD[2].1));
    }

    #[test]
    fn short_titles() {
        let standard = with_standard(Vec::new());
        assert_eq!(title(&standard[0]), "Code only in code blocks");
        let own = StartArg { text: "I use Arch Linux with KDE. Answer short.".into(), ..Default::default() };
        assert_eq!(title(&own), "I use Arch Linux with KDE");
        let long = StartArg { text: "word ".repeat(30), ..Default::default() };
        assert!(title(&long).ends_with('…') && title(&long).chars().count() <= TITLE_CHARS + 1);
    }

    #[test]
    fn group_extras_switch_on_global_off_args() {
        let list = with_standard(Vec::new());
        let off = list.iter().find(|arg| arg.standard == "no_code_comments").unwrap();
        assert!(!off.enabled);
        let mixed = for_group(&list, &["no_code_comments".to_string(), crate::answer_style::ID.to_string()], &[], true);
        assert!(mixed.iter().all(|arg| arg.enabled));
        let removed = for_group(&list, &[], &["honest".to_string()], true);
        assert!(!removed.iter().find(|arg| arg.standard == "honest").unwrap().enabled);
        assert!(removed.iter().find(|arg| arg.standard == "use_tools").unwrap().enabled);
        let only = for_group(&list, &["no_code_comments".to_string()], &[], false);
        assert_eq!(only.iter().filter(|arg| arg.enabled).count(), 1);
        assert!(system_note(&only).unwrap().contains("comments inside code"));
    }

    #[test]
    fn note_and_add() {
        let mut list = vec![StartArg {
            text: "I use Arch Linux".into(),
            enabled: true,
            ..Default::default()
        }];
        assert!(add(&mut list, "  ", false).is_err());
        assert!(add(&mut list, "i use arch linux", true).unwrap().starts_with("Already"));
        add(&mut list, "Answer short", true).unwrap();
        list[0].enabled = false;
        assert_eq!(
            system_note(&list).unwrap(),
            "Always follow these points:\n- Answer short"
        );
        list[1].enabled = false;
        assert!(system_note(&list).is_none());
    }
}
