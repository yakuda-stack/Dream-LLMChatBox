use crate::i18n::{tr, trf};
use crate::mcp::{self, ServerConfig, Status};
use crate::{AppWindow, McpEntry, McpPage};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::process::{Command, Stdio};
use std::thread;

pub fn setup(ui: &AppWindow) {
    refresh(ui);
    let weak = ui.as_weak();
    mcp::set_listener(move || {
        let _ = weak.upgrade_in_event_loop(|ui| refresh(&ui));
    });
    let page = ui.global::<McpPage>();

    let weak = ui.as_weak();
    page.on_toggle(move |name, on| {
        let name = name.to_string();
        let result = change(|list| {
            if let Some(server) = list.iter_mut().find(|s| s.name == name) {
                server.disabled = !on;
            }
            Ok(())
        });
        mcp::stop(&name);
        finish(&weak, result.err());
    });

    let weak = ui.as_weak();
    page.on_test(move |name| {
        let Some(config) = find(&name) else { return };
        let weak = weak.clone();
        thread::spawn(move || {
            let message = match mcp::restart(&config) {
                Status::Running(tools) => trf("{}: started, {} tool(s).", &[&config.name, &tools.len()]),
                Status::Failed(e) => format!("{}: {e}", config.name),
                _ => String::new(),
            };
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.global::<McpPage>().set_message(message.into());
                refresh(&ui);
            });
        });
    });

    let weak = ui.as_weak();
    page.on_edit(move |name| {
        let Some(ui) = weak.upgrade() else { return };
        let Some(config) = find(&name) else { return };
        let page = ui.global::<McpPage>();
        page.set_form_old_name(config.name.clone().into());
        page.set_form_name(config.name.into());
        page.set_form_command(config.command.into());
        page.set_form_args(mcp::join_args(&config.args).into());
        page.set_form_env(mcp::env_text(&config.env).into());
        page.set_form_error("".into());
        page.set_form_open(true);
    });

    let weak = ui.as_weak();
    page.on_delete(move |name| {
        let name = name.to_string();
        mcp::stop(&name);
        let result = change(|list| {
            list.retain(|s| s.name != name);
            Ok(())
        });
        finish(&weak, result.err());
    });

    let weak = ui.as_weak();
    page.on_save(move |old, name, command, args, env| {
        let Some(ui) = weak.upgrade() else { return };
        let page = ui.global::<McpPage>();
        match save(&old, &name, &command, &args, &env) {
            Ok(()) => {
                mcp::stop(&old);
                mcp::stop(name.trim());
                page.set_form_error("".into());
                page.set_form_open(false);
                page.set_message("".into());
                refresh(&ui);
            }
            Err(e) => page.set_form_error(e.into()),
        }
    });

    let weak = ui.as_weak();
    page.on_import_claude(move || {
        let from = mcp::claude_config_path();
        let message = match mcp::import_in(&mcp::config_path(), &from) {
            Err(e) => e,
            Ok(result) if result.added == 0 => trf(
                "Nothing new in {} ({} already there, {} without a command skipped).",
                &[&from.display(), &result.existing, &result.skipped],
            ),
            Ok(result) => trf(
                "Imported {} MCP server(s). They are switched off – check the command before you switch them on.",
                &[&result.added],
            ),
        };
        if let Some(ui) = weak.upgrade() {
            refresh(&ui);
            ui.global::<McpPage>().set_message(message.into());
        }
    });

    let weak = ui.as_weak();
    page.on_open_config(move || {
        let path = mcp::config_path();
        if !path.exists()
            && let Err(e) = mcp::save_in(&path, &[])
        {
            finish(&weak, Some(e));
            return;
        }
        let _ = Command::new("xdg-open")
            .arg(&path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    });
}

fn find(name: &str) -> Option<ServerConfig> {
    mcp::load().into_iter().find(|s| s.name == name)
}

fn change(edit: impl FnOnce(&mut Vec<ServerConfig>) -> Result<(), String>) -> Result<(), String> {
    let path = mcp::config_path();
    let mut list = mcp::load_in(&path)?;
    edit(&mut list)?;
    mcp::save_in(&path, &list)
}

fn save(old: &str, name: &str, command: &str, args: &str, env: &str) -> Result<(), String> {
    let name = name.trim();
    let command = command.trim();
    if name.is_empty() {
        return Err(tr("Please enter a name."));
    }
    if command.is_empty() {
        return Err(tr("Please enter a command."));
    }
    let args = mcp::split_args(args)?;
    let env = mcp::parse_env(env)?;
    change(|list| {
        if list.iter().any(|s| s.name == name && s.name != old) {
            return Err(trf("A server called \"{}\" already exists.", &[&name]));
        }
        let existing = list.iter().position(|s| s.name == old && !old.is_empty());
        let mut server = existing.map(|index| list.remove(index)).unwrap_or_default();
        server.name = name.to_string();
        server.command = command.to_string();
        server.args = args;
        server.env = env;
        list.push(server);
        Ok(())
    })
}

fn finish(weak: &slint::Weak<AppWindow>, error: Option<String>) {
    if let Some(ui) = weak.upgrade() {
        if let Some(e) = error {
            ui.global::<McpPage>().set_message(e.into());
        }
        refresh(&ui);
    }
}

fn entry(config: &ServerConfig) -> McpEntry {
    let (status, detail, failed, busy) = match mcp::status(&config.name) {
        Status::NotStarted if config.disabled => (tr("off"), String::new(), false, false),
        Status::NotStarted => (tr("not started"), String::new(), false, false),
        Status::Starting => (tr("starting…"), String::new(), false, true),
        Status::Running(tools) => (
            trf("running – {} tools", &[&tools.len()]),
            tools.join(", "),
            false,
            false,
        ),
        Status::Failed(e) => (tr("error:"), e, true, false),
    };
    let mut command = config.command.clone();
    if !config.args.is_empty() {
        command.push(' ');
        command.push_str(&mcp::join_args(&config.args));
    }
    McpEntry {
        name: config.name.clone().into(),
        command: command.into(),
        status: status.into(),
        detail: detail.into(),
        enabled: !config.disabled,
        failed,
        busy,
    }
}

pub fn refresh(ui: &AppWindow) {
    let page = ui.global::<McpPage>();
    let list = match mcp::load_in(&mcp::config_path()) {
        Ok(list) => list,
        Err(e) => {
            page.set_message(e.into());
            Vec::new()
        }
    };
    let entries: Vec<McpEntry> = list.iter().map(entry).collect();
    page.set_servers(ModelRc::new(VecModel::from(entries)));
}
