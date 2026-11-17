use std::fs;
use std::path::PathBuf;

pub const ID: &str = "answer_style";
pub const DEFAULT_CHOICE: &str = "claude";

const DEFAULTS: [(&str, &str, &str); 5] = [
    (
        "claude",
        "Claude (Code)",
        "Calm, direct and friendly. Lead with the result, then the reasons. Short paragraphs; lists only when they make things easier to scan. For code: say in one sentence what you change and why, then the code. At the end say briefly what you checked and what is still open. No flattery, no filler, no exaggeration; say plainly when you are not sure.",
    ),
    (
        "gemini",
        "Gemini",
        "Well structured: start with a one-line summary, then clear headings and bullet points. Use tables for comparisons. Add useful background where it helps, and end with short suggested next steps.",
    ),
    (
        "chatgpt",
        "ChatGPT",
        "Conversational and helpful. Explain step by step with numbered lists, put key terms in bold, give a short example when useful, and end with a friendly offer to help further.",
    ),
    (
        "grok",
        "Grok",
        "Casual, witty and direct, with a bit of humor and edge. Short punchy sentences, no corporate tone, no needless caveats – but stay correct and helpful.",
    ),
    (
        "deepseek",
        "DeepSeek",
        "Analytical and thorough. Work through the problem step by step, show the key reasoning briefly, then give a clear final answer. Use precise technical language; for math and code give complete, working solutions.",
    ),
];

pub struct Style {
    pub id: String,
    pub name: String,
    pub text: String,
}

pub fn dir() -> PathBuf {
    std::env::var_os("ANSWER_STYLES_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("dream-llmchatbox")
                .join("answer_styles")
        })
}

pub fn ensure_defaults() {
    let dir = dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    for (id, name, text) in DEFAULTS {
        let path = dir.join(format!("{id}.md"));
        if !path.exists() {
            let _ = fs::write(path, format!("# {name}\n\n{text}\n"));
        }
    }
}

pub fn parse(id: &str, content: &str) -> Style {
    let mut lines = content.lines();
    let first = lines.next().unwrap_or_default().trim();
    let (name, text) = match first.strip_prefix('#') {
        Some(name) => (name.trim().to_string(), lines.collect::<Vec<_>>().join("\n")),
        None => (id.to_string(), content.to_string()),
    };
    Style {
        id: id.to_string(),
        name: if name.is_empty() { id.to_string() } else { name },
        text: text.trim().to_string(),
    }
}

pub fn list() -> Vec<Style> {
    ensure_defaults();
    let mut styles: Vec<Style> = fs::read_dir(dir())
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
                .filter_map(|entry| {
                    let id = entry.path().file_stem()?.to_string_lossy().to_string();
                    let content = fs::read_to_string(entry.path()).ok()?;
                    Some(parse(&id, &content))
                })
                .collect()
        })
        .unwrap_or_default();
    let order = |id: &str| DEFAULTS.iter().position(|(d, _, _)| *d == id).unwrap_or(DEFAULTS.len());
    styles.sort_by(|a, b| order(&a.id).cmp(&order(&b.id)).then(a.name.cmp(&b.name)));
    styles
}

pub fn instruction(choice: &str) -> Option<String> {
    let styles = list();
    let style = styles
        .iter()
        .find(|style| style.id == choice)
        .or_else(|| styles.iter().find(|style| style.id == DEFAULT_CHOICE))?;
    (!style.text.is_empty()).then(|| {
        format!(
            "Answer in the style of {} (only the writing style – you stay yourself and never claim to be that assistant): {}",
            style.name, style.text
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_are_files_and_parsed() {
        let dir = std::env::temp_dir().join(format!("dlc-styles-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        unsafe { std::env::set_var("ANSWER_STYLES_DIR", &dir) };
        let styles = list();
        assert_eq!(styles.len(), 5);
        assert_eq!(styles[0].id, "claude");
        assert!(dir.join("grok.md").exists());
        fs::write(dir.join("pirate.md"), "# Pirate\n\nTalk like a pirate.").unwrap();
        assert!(instruction("pirate").unwrap().contains("Pirate"));
        assert!(instruction("missing").unwrap().contains("Claude"));
        let parsed = parse("x", "no heading here");
        assert_eq!(parsed.name, "x");
        unsafe { std::env::remove_var("ANSWER_STYLES_DIR") };
        let _ = fs::remove_dir_all(&dir);
    }
}
