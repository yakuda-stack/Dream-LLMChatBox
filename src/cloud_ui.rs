use crate::i18n::{tr, trf};
use crate::cloud_cli::{self, PROVIDERS};
use crate::settings;
use crate::{AppWindow, CloudEntry, CloudModels};
use slint::{ComponentHandle, ModelRc, VecModel};

pub fn active_names() -> Vec<String> {
    let active = settings::load().cloud_active;
    PROVIDERS
        .iter()
        .filter(|provider| active.iter().any(|id| id == provider.id) && cloud_cli::installed(provider))
        .map(cloud_cli::display_name)
        .collect()
}

fn status(provider: &cloud_cli::Provider) -> String {
    if !cloud_cli::installed(provider) {
        return tr("Not installed");
    }
    match cloud_cli::logged_in(provider) {
        Some(true) => tr("Installed · logged in"),
        Some(false) => tr("Installed · please log in once"),
        None => tr("Installed · log in once if you have not yet"),
    }
}

pub fn refresh(ui: &AppWindow) {
    let active = settings::load().cloud_active;
    let entries: Vec<CloudEntry> = PROVIDERS
        .iter()
        .map(|provider| CloudEntry {
            id: provider.id.into(),
            name: provider.name.into(),
            status: status(provider).into(),
            installed: cloud_cli::installed(provider),
            active: active.iter().any(|id| id == provider.id),
        })
        .collect();
    ui.global::<CloudModels>().set_entries(ModelRc::new(VecModel::from(entries)));
}

fn in_terminal(ui: &AppWindow, argv: &[String], done: &str) {
    let command = argv.iter().map(|part| shell_quote(part)).collect::<Vec<_>>().join(" ");
    let script = format!("cd ~ && {command}; echo; echo '{done}'; read -r _");
    let message = match crate::terminal::run(&script) {
        Ok(_) => tr("A terminal opened – when it is done, press “Check again”."),
        Err(e) => trf("{}. Run it yourself: {}", &[&e, &command]),
    };
    ui.global::<CloudModels>().set_message(message.into());
}

fn shell_quote(part: &str) -> String {
    if part.chars().all(|c| c.is_ascii_alphanumeric() || "-_./@=~:".contains(c)) && !part.is_empty() {
        part.to_string()
    } else {
        format!("'{}'", part.replace('\'', "'\\''"))
    }
}

pub fn setup(ui: &AppWindow) {
    refresh(ui);
    let state = ui.global::<CloudModels>();

    let weak = ui.as_weak();
    state.on_refresh(move || {
        if let Some(ui) = weak.upgrade() {
            refresh(&ui);
            ui.global::<CloudModels>().set_message("".into());
            ui.invoke_refresh_models();
        }
    });

    let weak = ui.as_weak();
    state.on_install(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        let Some(provider) = cloud_cli::provider(&id) else { return };
        match cloud_cli::install_command(provider) {
            Ok(argv) => in_terminal(&ui, &argv, "Done – press Enter, then “Check again” in Dream-LLMChatBox."),
            Err(e) => ui.global::<CloudModels>().set_message(e.into()),
        }
    });

    let weak = ui.as_weak();
    state.on_login(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        let Some(provider) = cloud_cli::provider(&id) else { return };
        match cloud_cli::login_command(provider) {
            Some(argv) => in_terminal(&ui, &argv, "Press Enter to close."),
            None => ui.global::<CloudModels>().set_message(tr("Install it first.").into()),
        }
    });

    let weak = ui.as_weak();
    state.on_toggle(move |id, on| {
        let Some(ui) = weak.upgrade() else { return };
        settings::update(|s| {
            s.cloud_active.retain(|active| *active != id.as_str());
            if on {
                s.cloud_active.push(id.to_string());
            }
        });
        refresh(&ui);
        ui.invoke_refresh_models();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_shell_parts() {
        assert_eq!(shell_quote("npm"), "npm");
        assert_eq!(shell_quote("curl -fsSL x | bash"), "'curl -fsSL x | bash'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }
}
