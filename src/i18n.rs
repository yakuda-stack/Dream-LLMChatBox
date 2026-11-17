use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

const GERMAN: &str = include_str!("../lang/de/LC_MESSAGES/dream-llmchatbox.po");

static IS_GERMAN: AtomicBool = AtomicBool::new(false);

fn unquote(line: &str) -> String {
    let line = line.trim();
    let inner = line.strip_prefix('"').unwrap_or(line);
    let inner = inner.strip_suffix('"').unwrap_or(inner);
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn parse(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut id = String::new();
    let mut value = String::new();
    let mut target = 0;
    let mut flush = |id: &mut String, value: &mut String| {
        if !id.is_empty() && !value.is_empty() {
            map.insert(std::mem::take(id), std::mem::take(value));
        }
        id.clear();
        value.clear();
    };
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("msgid ") {
            flush(&mut id, &mut value);
            id = unquote(rest);
            target = 1;
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            value = unquote(rest);
            target = 2;
        } else if line.starts_with('"') {
            match target {
                1 => id.push_str(&unquote(line)),
                2 => value.push_str(&unquote(line)),
                _ => {}
            }
        } else if line.is_empty() || line.starts_with('#') {
            target = 0;
        }
    }
    flush(&mut id, &mut value);
    map
}

fn catalog() -> &'static HashMap<String, String> {
    static CATALOG: OnceLock<HashMap<String, String>> = OnceLock::new();
    CATALOG.get_or_init(|| parse(GERMAN))
}

pub fn setup() {
    set_language(&crate::settings::load().ui_language);
}

pub fn set_language(code: &str) {
    IS_GERMAN.store(code == "de", Ordering::SeqCst);
    let _ = slint::select_bundled_translation(if code == "de" { "de" } else { "" });
}

pub fn refresh(ui: &crate::AppWindow) {
    crate::profile::set_option_labels(ui);
    crate::automations_ui::set_labels(ui);
    crate::automations_ui::refresh(ui);
    crate::chat::refresh_chat_list(ui);
}

pub fn language() -> &'static str {
    if IS_GERMAN.load(Ordering::SeqCst) { "de" } else { "en" }
}

pub fn tr(text: &str) -> String {
    if IS_GERMAN.load(Ordering::SeqCst)
        && let Some(found) = catalog().get(text)
    {
        return found.clone();
    }
    text.to_string()
}

pub fn trf(text: &str, args: &[&dyn Display]) -> String {
    let mut out = String::new();
    let mut rest = tr(text);
    for arg in args {
        match rest.find("{}") {
            Some(index) => {
                out.push_str(&rest[..index]);
                out.push_str(&arg.to_string());
                rest = rest[index + 2..].to_string();
            }
            None => break,
        }
    }
    out.push_str(&rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_formats() {
        let map = parse("msgid \"\"\nmsgstr \"x\"\n\nmsgid \"Hello {}\"\nmsgstr \"Hallo \"\n\"{}\"\n\nmsgid \"A \\\"b\\\"\"\nmsgstr \"C\"\n");
        assert_eq!(map.get("Hello {}").map(String::as_str), Some("Hallo {}"));
        assert_eq!(map.get("A \"b\"").map(String::as_str), Some("C"));
        assert_eq!(trf("Saved {} of {}", &[&1, &"x"]), "Saved 1 of x");
    }

    #[test]
    fn german_catalog_is_complete() {
        let map = catalog();
        assert!(map.len() > 700, "{}", map.len());
        for (id, text) in map {
            assert_eq!(id.matches("{}").count(), text.matches("{}").count(), "{id}");
        }
        assert_eq!(map.get("New chat").map(String::as_str), Some("Neuer Chat"));
    }
}
