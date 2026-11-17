use crate::i18n::tr;
use crate::AppWindow;
use slint::ComponentHandle;
use std::fs;
use std::path::PathBuf;

pub fn targets() -> Vec<PathBuf> {
    let mut list = vec![crate::storage::base_dir(), crate::workspace::base()];
    if let Some(config) = dirs::config_dir() {
        list.push(config.join("dream-llmchatbox"));
    }
    list.retain(|path| path.exists() && is_safe(path));
    list
}

fn is_safe(path: &std::path::Path) -> bool {
    let home = dirs::home_dir().unwrap_or_default();
    path.components().count() > 2 && path != home && !home.starts_with(path)
}

pub fn summary() -> String {
    targets()
        .iter()
        .map(|path| format!("• {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn setup(ui: &AppWindow) {
    let weak = ui.as_weak();
    ui.on_open_reset(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_reset_folders(summary().into());
            ui.set_settings_page(0);
            ui.set_reset_open(true);
        }
    });

    let weak = ui.as_weak();
    ui.on_reset_all(move || {
        if let Some(ui) = weak.upgrade() {
            reset_all(&ui);
        }
    });
}

pub fn reset_all(ui: &AppWindow) {
    let list = targets();
    crate::storage::close();
    crate::mcp::shutdown();
    crate::server::stop_own();
    let mut errors = Vec::new();
    for path in &list {
        if let Err(e) = fs::remove_dir_all(path) {
            errors.push(format!("{}: {e}", path.display()));
        }
    }
    if !errors.is_empty() {
        ui.set_status(format!("{} {}", tr("Reset incomplete:"), errors.join("; ")).into());
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_deletes_home_or_root() {
        let home = dirs::home_dir().unwrap();
        assert!(!is_safe(&home));
        assert!(!is_safe(std::path::Path::new("/")));
        assert!(!is_safe(home.parent().unwrap()));
        assert!(is_safe(&home.join(".local/share/dream-llmchatbox")));
    }
}
