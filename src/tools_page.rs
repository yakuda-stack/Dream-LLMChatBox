use crate::i18n::{tr, trf};
use crate::custom_tools::{self, CustomTool};
use crate::settings;
use crate::tools;
use crate::{AppWindow, ToolEntry};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::fs;
use std::process::{Command, Stdio};
use std::thread;

pub fn setup(ui: &AppWindow) {
    migrate_descriptions();
    refresh(ui);

    let weak = ui.as_weak();
    ui.on_save_safe_commands(move |text| {
        let list: Vec<String> = text
            .lines()
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|line| !line.is_empty())
            .collect();
        settings::update(|s| s.safe_commands = list);
        if let Some(ui) = weak.upgrade() {
            ui.set_tools_message(tr("Safe commands saved.").into());
            refresh(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_reset_safe_commands(move || {
        settings::update(|s| s.safe_commands = settings::default_safe_commands());
        if let Some(ui) = weak.upgrade() {
            refresh(&ui);
        }
    });

    ui.on_tools_open_folder(|| {
        let dir = custom_tools::tools_dir();
        let _ = fs::create_dir_all(&dir);
        let _ = Command::new("xdg-open")
            .arg(&dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    });

    let weak = ui.as_weak();
    ui.on_tools_import(move || import(weak.clone()));
    let weak = ui.as_weak();
    ui.on_tools_export(move || export(weak.clone()));

    let weak = ui.as_weak();
    ui.on_tools_global(move |on| {
        settings::update(|s| s.tools_enabled = on);
        if let Some(ui) = weak.upgrade() {
            ui.set_tools_enabled(on);
        }
    });

    let weak = ui.as_weak();
    ui.on_tool_toggle(move |name, on| {
        let name = name.to_string();
        if is_builtin(&name) {
            settings::update(|s| {
                s.disabled_tools.retain(|off| *off != name);
                if !on {
                    s.disabled_tools.push(name.clone());
                }
            });
        } else {
            let mut list = custom_tools::load();
            if let Some(tool) = list.iter_mut().find(|tool| tool.name == name.as_str()) {
                tool.enabled = on;
            }
            let _ = custom_tools::save(&list);
        }
        if let Some(ui) = weak.upgrade() {
            refresh(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_tool_edit(move |name| {
        let Some(ui) = weak.upgrade() else { return };
        let file = custom_tools::load().into_iter().find(|tool| tool.name == name.as_str());
        if is_builtin(&name) {
            let tool = file.unwrap_or_else(|| CustomTool {
                name: name.to_string(),
                description: tools::default_description(&name),
                params: tools::default_params(&name),
                ..Default::default()
            });
            ui.set_tool_form_builtin(true);
            ui.set_tool_form_old_name(name.clone());
            ui.set_tool_form_name(name);
            ui.set_tool_form_description(tool.description.into());
            ui.set_tool_form_params(tool.params.join(", ").into());
            ui.set_tool_form_command(tool.command.into());
            ui.set_tool_form_error("".into());
            ui.set_tool_form_open(true);
            return;
        }
        ui.set_tool_form_builtin(false);
        if let Some(tool) = file {
            ui.set_tool_form_old_name(tool.name.clone().into());
            ui.set_tool_form_name(tool.name.into());
            ui.set_tool_form_description(tool.description.into());
            ui.set_tool_form_params(tool.params.join(", ").into());
            ui.set_tool_form_command(tool.command.into());
            ui.set_tool_form_error("".into());
            ui.set_tool_form_open(true);
        }
    });

    let weak = ui.as_weak();
    ui.on_tool_save(move |old, name, description, params, command| {
        let Some(ui) = weak.upgrade() else { return };
        if is_builtin(&old) {
            match save_builtin(&old, description.trim(), &params, command.trim()) {
                Ok(()) => {
                    ui.set_tool_form_error("".into());
                    ui.set_tool_form_open(false);
                    refresh(&ui);
                }
                Err(e) => ui.set_tool_form_error(e.into()),
            }
            return;
        }
        let tool = CustomTool {
            name: name.trim().to_string(),
            description: description.trim().to_string(),
            params: custom_tools::parse_params(&params),
            command: command.trim().to_string(),
            enabled: true,
        };
        match save_tool(old.as_str(), tool) {
            Ok(()) => {
                ui.set_tool_form_error("".into());
                ui.set_tool_form_open(false);
                refresh(&ui);
            }
            Err(e) => ui.set_tool_form_error(e.into()),
        }
    });

    let weak = ui.as_weak();
    ui.on_tool_reset(move |name| {
        let mut list = custom_tools::load();
        list.retain(|tool| tool.name != name.as_str());
        let _ = custom_tools::save(&list);
        if let Some(ui) = weak.upgrade() {
            refresh(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_tool_delete(move |name| {
        let mut list = custom_tools::load();
        list.retain(|tool| tool.name != name.as_str());
        let _ = custom_tools::save(&list);
        if let Some(ui) = weak.upgrade() {
            refresh(&ui);
        }
    });
}

fn is_builtin(name: &str) -> bool {
    tools::is_builtin(name)
}

fn save_builtin(name: &str, description: &str, params: &str, command: &str) -> Result<(), String> {
    let default_description = tools::default_description(name);
    let tool = CustomTool {
        name: name.to_string(),
        description: if description.is_empty() {
            default_description.clone()
        } else {
            description.to_string()
        },
        params: if command.is_empty() {
            tools::default_params(name)
        } else {
            custom_tools::parse_params(params)
        },
        command: command.to_string(),
        enabled: true,
    };
    custom_tools::validate_fields(&tool, true)?;
    let mut list = custom_tools::load();
    list.retain(|t| t.name != name);
    if tool.description != default_description || !tool.command.is_empty() {
        list.push(tool);
    }
    custom_tools::save(&list)
}

fn migrate_descriptions() {
    let old = settings::load().tool_descriptions;
    if old.is_empty() {
        return;
    }
    let mut list = custom_tools::load();
    for (name, text) in old {
        if is_builtin(&name) && !list.iter().any(|t| t.name == name) {
            list.push(CustomTool {
                params: tools::default_params(&name),
                name,
                description: text,
                ..Default::default()
            });
        }
    }
    if custom_tools::save(&list).is_ok() {
        settings::update(|s| s.tool_descriptions.clear());
    }
}

fn builtin_names() -> Vec<String> {
    tools::BUILTIN.iter().map(|(name, _)| name.to_string()).collect()
}

fn import(weak: slint::Weak<AppWindow>) {
    thread::spawn(move || {
        let Some(path) = rfd::FileDialog::new()
            .set_title(tr("Import tools"))
            .add_filter(tr("Tools (JSON)"), &["json"])
            .pick_file()
        else {
            return;
        };
        let message = match fs::read_to_string(&path) {
            Err(e) => trf("Could not read {}: {}", &[&path.display(), &e]),
            Ok(text) => {
                let mut list = custom_tools::load();
                match custom_tools::import_json(&text, &mut list, &builtin_names()) {
                    Ok(0) => tr("Nothing new – all tools in this file already exist."),
                    Ok(count) => match custom_tools::save(&list) {
                        Ok(()) => trf(
                            "Imported {} tool(s). They are switched off – check the command before you switch them on.",
                            &[&count],
                        ),
                        Err(e) => trf("Could not save: {}", &[&e]),
                    },
                    Err(e) => e,
                }
            }
        };
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.set_tools_message(message.into());
            refresh(&ui);
        });
    });
}

fn export(weak: slint::Weak<AppWindow>) {
    thread::spawn(move || {
        let list: Vec<CustomTool> = custom_tools::load()
            .into_iter()
            .filter(|tool| !is_builtin(&tool.name))
            .collect();
        if list.is_empty() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title(tr("Export tools"))
            .set_file_name("dream-llmchatbox-tools.json")
            .save_file()
        else {
            return;
        };
        let message = match fs::write(&path, custom_tools::export_json(&list)) {
            Ok(()) => trf("Exported {} tool(s) to {}", &[&list.len(), &path.display()]),
            Err(e) => trf("Could not write {}: {}", &[&path.display(), &e]),
        };
        let _ = weak.upgrade_in_event_loop(move |ui| ui.set_tools_message(message.into()));
    });
}

fn save_tool(old: &str, mut tool: CustomTool) -> Result<(), String> {
    let mut list = custom_tools::load();
    let taken: Vec<String> = builtin_names()
        .into_iter()
        .chain(list.iter().map(|t| t.name.clone()).filter(|name| name != old))
        .collect();
    custom_tools::validate(&tool, &taken)?;
    match list.iter_mut().find(|t| t.name == old && !old.is_empty()) {
        Some(existing) => {
            tool.enabled = existing.enabled;
            *existing = tool;
        }
        None => list.push(tool),
    }
    custom_tools::save(&list)
}

fn detail(tool: &CustomTool) -> String {
    if tool.command.is_empty() {
        String::new()
    } else if tool.params.is_empty() {
        format!("$ {}", tool.command)
    } else {
        format!("({})  $ {}", tool.params.join(", "), tool.command)
    }
}

fn refresh(ui: &AppWindow) {
    let current = settings::load();
    ui.set_tools_enabled(current.tools_enabled);
    ui.set_safe_commands(current.safe_commands.join("\n").into());
    let files = custom_tools::load();
    let builtin: Vec<ToolEntry> = tools::BUILTIN
        .iter()
        .map(|(name, description)| {
            let file = files.iter().find(|tool| tool.name == *name);
            ToolEntry {
                name: (*name).into(),
                description: if *name == crate::documents::NAME {
                    format!("{description}  ({})", crate::documents::status()).into()
                } else {
                    (*description).into()
                },
                detail: file.map(detail).unwrap_or_default().into(),
                enabled: !current.disabled_tools.iter().any(|off| off == name),
                custom: false,
                edited: file.is_some(),
            }
        })
        .collect();
    let custom: Vec<ToolEntry> = files
        .iter()
        .filter(|tool| !is_builtin(&tool.name))
        .map(|tool| ToolEntry {
            detail: detail(tool).into(),
            name: tool.name.clone().into(),
            description: tool.description.clone().into(),
            enabled: tool.enabled,
            custom: true,
            edited: false,
        })
        .collect();
    ui.set_builtin_tools(ModelRc::new(VecModel::from(builtin)));
    ui.set_custom_tools(ModelRc::new(VecModel::from(custom)));
}
