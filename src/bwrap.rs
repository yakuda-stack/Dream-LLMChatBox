use crate::groups;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const WRITABLE_CACHES: [&str; 4] = [".cargo/registry", ".cargo/git", ".cache", ".npm"];
const HIDDEN: [&str; 4] = [".ssh", ".gnupg", ".local/share/keyrings", ".password-store"];
pub const READ_ONLY_HINT: &str = "[Sandbox: only the shared project folder is writable; the rest of the system is read-only.]";

fn path() -> PathBuf {
    crate::storage::data_dir().join("chat_sandbox.json")
}

fn load_in(file: &Path) -> HashMap<String, bool> {
    fs::read_to_string(file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_in(file: &Path, map: &HashMap<String, bool>) {
    if let Some(parent) = file.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(map) {
        let _ = fs::write(file, text);
    }
}

fn set_in(file: &Path, chat: &str, choice: Option<bool>) {
    let mut map = load_in(file);
    match choice {
        Some(on) => map.insert(chat.to_string(), on),
        None => map.remove(chat),
    };
    save_in(file, &map);
}

pub fn installed() -> bool {
    crate::documents::which("bwrap")
}

pub fn group_on(group: &str) -> bool {
    group.is_empty() || groups::load(group).is_none_or(|group| !group.sandbox_off)
}

pub fn enabled(chat: &str, group: &str) -> bool {
    load_in(&path()).get(chat).copied().unwrap_or_else(|| group_on(group))
}

pub fn set_chat(chat: &str, group: &str, on: bool) {
    if chat.is_empty() {
        return;
    }
    let choice = (on != group_on(group)).then_some(on);
    set_in(&path(), chat, choice);
}

pub fn forget_chat(chat: &str) {
    set_in(&path(), chat, None);
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub fn script(home: Option<&Path>, directory: &Path, writable: bool, command: &str) -> String {
    let dir = quote(&directory.to_string_lossy());
    let mut parts = vec![
        "bwrap --ro-bind / / --dev /dev --unshare-pid --proc /proc --tmpfs /tmp".to_string(),
    ];
    if let Some(home) = home {
        for cache in WRITABLE_CACHES {
            let path = home.join(cache);
            if path.is_dir() {
                let path = quote(&path.to_string_lossy());
                parts.push(format!("--bind {path} {path}"));
            }
        }
        for secret in HIDDEN {
            let path = home.join(secret);
            if path.is_dir() {
                parts.push(format!("--tmpfs {}", quote(&path.to_string_lossy())));
            }
        }
    }
    let bind = if writable { "--bind" } else { "--ro-bind" };
    parts.push(format!("{bind} {dir} {dir} --chdir {dir} --die-with-parent --new-session bash -c {}", quote(command)));
    parts.join(" ")
}

pub fn wrap(chat: &str, group: &str, directory: &Path, writable: bool, command: &str) -> String {
    if !enabled(chat, group) || !installed() {
        return command.to_string();
    }
    script(dirs::home_dir().as_deref(), directory, writable, command)
}

pub fn status() -> String {
    if installed() {
        crate::i18n::tr("bwrap found ✓ – commands can only write into the shared project folder.")
    } else {
        crate::i18n::tr("bwrap is not installed – commands run without sandbox. Install the package “bubblewrap”.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_command() {
        let text = script(None, Path::new("/tmp/my dir"), true, "echo 'hi'");
        assert_eq!(
            text,
            "bwrap --ro-bind / / --dev /dev --unshare-pid --proc /proc --tmpfs /tmp --bind '/tmp/my dir' '/tmp/my dir' --chdir '/tmp/my dir' --die-with-parent --new-session bash -c 'echo '\\''hi'\\'''"
        );
        assert!(script(None, Path::new("/a"), false, "ls").contains("--ro-bind '/a' '/a'"));
    }

    #[test]
    fn chat_choice_overrides_only_when_different() {
        let dir = std::env::temp_dir().join(format!("dream-bwrap-{}", std::process::id()));
        let file = dir.join("chat_sandbox.json");
        set_in(&file, "a", Some(false));
        assert_eq!(load_in(&file).get("a"), Some(&false));
        set_in(&file, "a", None);
        assert!(load_in(&file).is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}
