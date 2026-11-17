use crate::i18n::{tr, trf};
use crate::openai_compat::{self, PRESETS, Server};
use crate::settings;
use crate::{AppWindow, ServerEntry, ServerModels};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static STATUS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

pub fn active_servers() -> Vec<Server> {
    settings::load().servers.into_iter().filter(|server| server.enabled).collect()
}

pub fn refresh(ui: &AppWindow) {
    let entries: Vec<ServerEntry> = settings::load()
        .servers
        .iter()
        .map(|server| ServerEntry {
            name: server.name.as_str().into(),
            url: server.url.as_str().into(),
            status: STATUS.with(|status| status.borrow().get(&server.name).cloned().unwrap_or_default()).into(),
            enabled: server.enabled,
        })
        .collect();
    let state = ui.global::<ServerModels>();
    state.set_entries(ModelRc::new(VecModel::from(entries)));
    let presets: Vec<SharedString> = PRESETS.iter().map(|(name, _)| SharedString::from(*name)).collect();
    state.set_presets(ModelRc::new(VecModel::from(presets)));
}

pub fn setup(ui: &AppWindow) {
    refresh(ui);
    let state = ui.global::<ServerModels>();

    let weak = ui.as_weak();
    state.on_preset(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        if let Some((name, url)) = PRESETS.get(index as usize) {
            let state = ui.global::<ServerModels>();
            state.set_form_name((*name).into());
            state.set_form_url((*url).into());
        }
    });

    let weak = ui.as_weak();
    state.on_add(move || {
        let Some(ui) = weak.upgrade() else { return };
        let state = ui.global::<ServerModels>();
        let name = state.get_form_name().trim().replace(':', " ");
        let url = state.get_form_url().trim().to_string();
        if name.is_empty() || url.is_empty() {
            return;
        }
        if settings::load().servers.iter().any(|server| server.name == name) {
            state.set_message(tr("A server with this name already exists.").into());
            return;
        }
        let server = Server {
            name,
            url,
            api_key: state.get_form_key().trim().to_string(),
            enabled: true,
            tools: true,
        };
        settings::update(|s| s.servers.push(server.clone()));
        state.set_form_name("".into());
        state.set_form_url("".into());
        state.set_form_key("".into());
        state.set_message("".into());
        test(&ui, server);
    });

    let weak = ui.as_weak();
    state.on_remove(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        settings::update(|s| {
            if (index as usize) < s.servers.len() {
                s.servers.remove(index as usize);
            }
        });
        refresh(&ui);
        ui.invoke_refresh_models();
    });

    let weak = ui.as_weak();
    state.on_toggle(move |index, on| {
        let Some(ui) = weak.upgrade() else { return };
        settings::update(|s| {
            if let Some(server) = s.servers.get_mut(index as usize) {
                server.enabled = on;
            }
        });
        refresh(&ui);
        ui.invoke_refresh_models();
    });

    let weak = ui.as_weak();
    state.on_test(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        if let Some(server) = settings::load().servers.get(index as usize).cloned() {
            test(&ui, server);
        }
    });
}

fn test(ui: &AppWindow, server: Server) {
    STATUS.with(|status| status.borrow_mut().insert(server.name.clone(), tr("testing…")));
    refresh(ui);
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let result = openai_compat::list_models(&server);
        let _ = weak.upgrade_in_event_loop(move |ui| {
            let text = match result {
                Ok(models) if models.is_empty() => tr("reachable, but no models loaded"),
                Ok(models) => trf("✓ {} models", &[&models.len()]),
                Err(e) => format!("✗ {e}"),
            };
            STATUS.with(|status| status.borrow_mut().insert(server.name.clone(), text));
            refresh(&ui);
            ui.invoke_refresh_models();
        });
    });
}
