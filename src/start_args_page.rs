use crate::start_args;
use crate::{AppWindow, StartArgEntry};
use crate::answer_style;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

pub fn setup(ui: &AppWindow) {
    refresh(ui);

    let weak = ui.as_weak();
    ui.on_arg_toggle(move |index, on| {
        change(&weak, |list| {
            if let Some(arg) = list.get_mut(index as usize) {
                arg.enabled = on;
            }
        });
    });

    let weak = ui.as_weak();
    ui.on_arg_save(move |index, text| {
        let text = text.trim().to_string();
        change(&weak, |list| {
            if text.is_empty() {
                return;
            }
            match list.get_mut(index as usize).filter(|_| index >= 0) {
                Some(arg) => {
                    arg.text = text;
                    arg.by_model = false;
                }
                None => {
                    let _ = start_args::add(list, &text, false);
                }
            }
        });
    });

    let weak = ui.as_weak();
    ui.on_arg_delete(move |index| {
        change(&weak, |list| {
            if list.get(index as usize).is_some_and(|arg| arg.standard.is_empty()) {
                list.remove(index as usize);
            }
        });
    });

    let weak = ui.as_weak();
    ui.on_arg_choose(move |index, choice| {
        let styles = answer_style::list();
        let Some(style) = styles.get(choice as usize) else { return };
        let id = style.id.clone();
        change(&weak, |list| {
            if let Some(arg) = list.get_mut(index as usize) {
                arg.choice = id;
            }
        });
    });

    ui.on_arg_open_styles(|| {
        answer_style::ensure_defaults();
        let _ = std::process::Command::new("xdg-open")
            .arg(answer_style::dir())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    });

    let weak = ui.as_weak();
    ui.on_arg_reset(move |index| {
        change(&weak, |list| {
            if let Some(arg) = list.get_mut(index as usize)
                && let Some(text) = start_args::standard_text(&arg.standard)
            {
                arg.text = text.to_string();
            }
        });
    });
}

fn change(weak: &slint::Weak<AppWindow>, edit: impl FnOnce(&mut Vec<start_args::StartArg>)) {
    let mut list = start_args::load();
    edit(&mut list);
    let result = start_args::save(&list);
    if let Some(ui) = weak.upgrade() {
        if let Err(e) = result {
            ui.set_status(crate::i18n::trf("Could not save start arguments: {}", &[&e]).into());
        }
        refresh(&ui);
    }
}

pub fn refresh(ui: &AppWindow) {
    let styles = answer_style::list();
    let names: Vec<SharedString> = styles.iter().map(|style| style.name.clone().into()).collect();
    let entries: Vec<StartArgEntry> = start_args::load()
        .into_iter()
        .map(|arg| {
            let original = start_args::standard_text(&arg.standard);
            let has_choices = arg.standard == answer_style::ID;
            let choice_index = styles
                .iter()
                .position(|style| style.id == arg.choice)
                .unwrap_or(0) as i32;
            StartArgEntry {
                has_choices,
                choices: if has_choices {
                    ModelRc::new(VecModel::from(names.clone()))
                } else {
                    ModelRc::default()
                },
                choice_index,
                standard: original.is_some(),
                changed: original.is_some_and(|text| text != arg.text),
                title: start_args::title(&arg).into(),
                text: arg.text.into(),
                enabled: arg.enabled,
                by_model: arg.by_model,
            }
        })
        .collect();
    ui.set_start_args(ModelRc::new(VecModel::from(entries)));
}
