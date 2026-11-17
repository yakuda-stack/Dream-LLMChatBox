use std::fs;
use std::path::PathBuf;

pub const APP_ID: &str = "dream-llmchatbox";
const ICON: &[u8] = include_bytes!("../assets/icon.png");

fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
}

pub fn entry_text(exec: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Dream-LLMChatBox\nComment=Chat with local AI models (Ollama)\nExec=\"{exec}\"\nIcon={APP_ID}\nTerminal=false\nCategories=Utility;Development;\nStartupWMClass={APP_ID}\n"
    )
}

const MARK: &str = "Comment=Chat with local AI models (Ollama)";

pub fn exec_path() -> Option<String> {
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return Some(appimage.to_string_lossy().into_owned());
    }
    let exe = std::env::current_exe().ok()?;
    if exe.starts_with("/usr") {
        return None;
    }
    Some(exe.to_string_lossy().into_owned())
}

pub fn install() {
    let _ = slint::set_xdg_app_id(APP_ID);
    let Some(base) = data_home() else { return };
    let mut changed = false;
    let icon = base.join("icons/hicolor/256x256/apps").join(format!("{APP_ID}.png"));
    if fs::read(&icon).ok().as_deref() != Some(ICON) {
        let _ = icon.parent().map(fs::create_dir_all);
        let _ = fs::write(&icon, ICON);
        changed = true;
    }
    let desktop = base.join("applications").join(format!("{APP_ID}.desktop"));
    let Some(exec) = exec_path() else {
        if fs::read_to_string(&desktop).is_ok_and(|text| text.contains(MARK)) {
            let _ = fs::remove_file(&desktop);
            refresh_caches(&base);
        }
        return;
    };
    let text = entry_text(&exec);
    if fs::read_to_string(&desktop).ok().as_deref() != Some(text.as_str()) {
        let _ = desktop.parent().map(fs::create_dir_all);
        let _ = fs::write(&desktop, text);
        changed = true;
    }
    if changed {
        refresh_caches(&base);
    }
}

fn refresh_caches(base: &std::path::Path) {
    let applications = base.join("applications");
    let icons = base.join("icons/hicolor");
    std::thread::spawn(move || {
        let run = |program: &str, args: &[&std::ffi::OsStr]| {
            let _ = std::process::Command::new(program)
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        };
        run("update-desktop-database", &[applications.as_os_str()]);
        run("gtk-update-icon-cache", &[std::ffi::OsStr::new("-f"), std::ffi::OsStr::new("-t"), icons.as_os_str()]);
        for program in ["kbuildsycoca6", "kbuildsycoca5"] {
            run(program, &[]);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_matches_app_id() {
        let text = entry_text("/opt/dream/dream-llmchatbox");
        assert!(text.contains("Icon=dream-llmchatbox"));
        assert!(text.contains("StartupWMClass=dream-llmchatbox"));
        assert!(text.contains("Exec=\"/opt/dream/dream-llmchatbox\""));
        assert!(ICON.starts_with(b"\x89PNG"));
    }
}
