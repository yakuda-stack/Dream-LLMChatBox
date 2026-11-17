use crate::web_tools::{self, PageView};
use crate::{AppWindow, BrowserLink};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::process::{Command, Stdio};
use std::thread;

const MAX_HISTORY: usize = 30;
const MAX_SHOWN_CHARS: usize = 80_000;

thread_local! {
    static HISTORY: RefCell<Vec<PageView>> = const { RefCell::new(Vec::new()) };
}

pub fn setup(ui: &AppWindow) {
    let weak = ui.as_weak();
    web_tools::set_viewer(move |page| {
        let _ = weak.upgrade_in_event_loop(move |ui| {
            show(&ui, page, true);
            if !ui.get_side_open() {
                ui.invoke_open_side(0);
            }
        });
    });

    let weak = ui.as_weak();
    ui.on_browser_go(move |input| {
        let Some(ui) = weak.upgrade() else { return };
        let input = input.trim().to_string();
        if input.is_empty() || ui.get_browser_loading() {
            return;
        }
        ui.set_browser_address(input.clone().into());
        ui.set_browser_loading(true);
        ui.set_browser_error("".into());
        let weak = weak.clone();
        thread::spawn(move || {
            let result = web_tools::user_open(&input);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_browser_loading(false);
                match result {
                    Ok(page) => show(&ui, page, true),
                    Err(e) => ui.set_browser_error(e.into()),
                }
            });
        });
    });

    let weak = ui.as_weak();
    ui.on_browser_back(move || {
        let Some(ui) = weak.upgrade() else { return };
        let previous = HISTORY.with(|history| {
            let mut history = history.borrow_mut();
            if history.len() > 1 {
                history.pop();
            }
            history.last().cloned()
        });
        if let Some(page) = previous {
            show(&ui, page, false);
        }
    });

    let weak = ui.as_weak();
    ui.on_browser_external(move || {
        if let Some(ui) = weak.upgrade() {
            let url = ui.get_browser_url().to_string();
            if url.starts_with("http://") || url.starts_with("https://") {
                let _ = Command::new("xdg-open")
                    .arg(url)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
        }
    });
}

fn show(ui: &AppWindow, page: PageView, remember: bool) {
    let mut text: String = page.text.chars().take(MAX_SHOWN_CHARS).collect();
    if page.text.chars().count() > MAX_SHOWN_CHARS {
        text.push_str(&crate::i18n::tr("\n\n[… page cut for display]"));
    }
    let links: Vec<BrowserLink> = page
        .links
        .iter()
        .map(|(title, url)| BrowserLink {
            title: title.clone().into(),
            url: url.clone().into(),
        })
        .collect();
    ui.set_browser_url(page.url.clone().into());
    ui.set_browser_address(page.url.clone().into());
    ui.set_browser_title(page.title.clone().into());
    ui.set_browser_text(text.into());
    ui.set_browser_links(ModelRc::new(VecModel::from(links)));
    ui.set_browser_by_model(page.by_model);
    ui.set_browser_error("".into());
    let can_back = HISTORY.with(|history| {
        let mut history = history.borrow_mut();
        if remember {
            history.push(page);
            if history.len() > MAX_HISTORY {
                history.remove(0);
            }
        }
        history.len() > 1
    });
    ui.set_browser_can_back(can_back);
}
