use crate::i18n::{tr, trf};
use crate::catalog::{self, CatalogModel};
use crate::hardware::{self, format_gb, format_size};
use crate::ollama::{self, InstalledModel, TestResult};
use crate::online::{self, OnlineModel, OnlineVariant};
use crate::storage;
use crate::{AppWindow, AvailableRow, CatalogEntry, InstalledEntry, ModelVariant};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::thread;

const CATEGORIES: [&str; 6] = ["", "everyday", "coding", "reasoning", "vision", "all-in-one"];

#[derive(Clone, Copy, PartialEq)]
enum Source {
    Curated,
    Ollama,
    HuggingFace,
}

impl Source {
    fn from_index(index: i32) -> Self {
        match index {
            1 => Source::Ollama,
            2 => Source::HuggingFace,
            _ => Source::Curated,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Source::Curated => "Curated list",
            Source::Ollama => "ollama.com",
            Source::HuggingFace => "Hugging Face",
        }
    }
}

#[derive(Clone)]
struct VariantInfo {
    tag: String,
    label: String,
    size: u64,
    estimated: bool,
}

#[derive(Clone)]
struct Item {
    key: String,
    source: Source,
    id: String,
    group: String,
    description: String,
    capabilities: Vec<String>,
    categories: Vec<String>,
    strengths: Vec<String>,
    weaknesses: Vec<String>,
    variants: Vec<VariantInfo>,
    variants_loaded: bool,
    variants_error: Option<String>,
    downloads: String,
}

struct State {
    source: Source,
    query: String,
    category: usize,
    curated: Vec<Item>,
    online: Vec<Item>,
    expanded: HashSet<String>,
    selected: Option<String>,
    loading: bool,
    message: String,
    installed: Vec<String>,
    generation: u64,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State {
        source: Source::Curated,
        query: String::new(),
        category: 0,
        curated: curated_items(),
        online: Vec::new(),
        expanded: HashSet::new(),
        selected: None,
        loading: false,
        message: String::new(),
        installed: Vec::new(),
        generation: 0,
    });
}

pub fn setup(ui: &AppWindow) {
    ui.set_hardware_text(hardware::get().summary().into());
    update(ui, &[]);

    let weak = ui.as_weak();
    ui.on_install_model(move |tag| {
        if let Some(ui) = weak.upgrade() {
            install(&ui, &tag);
        }
    });

    let weak = ui.as_weak();
    ui.on_remove_model(move |name| {
        if let Some(ui) = weak.upgrade() {
            remove(&ui, name.to_string());
        }
    });

    let weak = ui.as_weak();
    ui.on_test_model(move |name| {
        if let Some(ui) = weak.upgrade() {
            test(&ui, name.to_string());
        }
    });

    let weak = ui.as_weak();
    ui.on_available_search(move |query| {
        if let Some(ui) = weak.upgrade() {
            STATE.with(|s| s.borrow_mut().query = query.trim().to_string());
            search(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_available_source(move |index| {
        if let Some(ui) = weak.upgrade() {
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                state.source = Source::from_index(index);
                state.online.clear();
                state.selected = None;
                state.message.clear();
            });
            search(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_available_category(move |index| {
        if let Some(ui) = weak.upgrade() {
            STATE.with(|s| s.borrow_mut().category = index.max(0) as usize);
            render(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_available_toggle_group(move |group| {
        if let Some(ui) = weak.upgrade() {
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                let group = group.to_string();
                if !state.expanded.remove(&group) {
                    state.expanded.insert(group);
                }
            });
            render(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_available_select(move |key| {
        if let Some(ui) = weak.upgrade() {
            select(&ui, key.to_string());
        }
    });

    render(ui);
}

fn curated_items() -> Vec<Item> {
    catalog::load().into_iter().map(curated_item).collect()
}

fn curated_item(model: CatalogModel) -> Item {
    Item {
        key: format!("curated:{}", model.name),
        source: Source::Curated,
        variants: model
            .variants
            .iter()
            .map(|v| VariantInfo {
                tag: v.tag.clone(),
                label: v.label().to_string(),
                size: v.size_bytes(),
                estimated: false,
            })
            .collect(),
        id: model.name,
        group: model.group,
        description: model.description,
        capabilities: model.capabilities,
        categories: model.categories,
        strengths: model.strengths,
        weaknesses: model.weaknesses,
        variants_loaded: true,
        variants_error: None,
        downloads: String::new(),
    }
}

fn online_item(source: Source, model: OnlineModel) -> Item {
    let short_name = model.id.rsplit('/').next().unwrap_or(&model.id).to_string();
    let categories = guess_categories(&short_name, &model.capabilities);
    let variants = match source {
        Source::Ollama => model
            .param_sizes
            .iter()
            .filter_map(|size| {
                let params = hardware::parse_params(size)?;
                Some(VariantInfo {
                    tag: format!("{}:{}", model.id, size),
                    label: size.clone(),
                    size: hardware::size_from_params(params),
                    estimated: true,
                })
            })
            .collect(),
        _ => Vec::new(),
    };
    Item {
        key: format!(
            "{}:{}",
            if source == Source::Ollama { "ollama" } else { "hf" },
            model.id
        ),
        source,
        group: family(&short_name),
        id: model.id,
        description: model.description,
        capabilities: model.capabilities,
        categories,
        strengths: Vec::new(),
        weaknesses: Vec::new(),
        variants,
        variants_loaded: false,
        variants_error: None,
        downloads: model.downloads,
    }
}

fn family(name: &str) -> String {
    let letters: String = name
        .to_lowercase()
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let mut chars = letters.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Other".to_string(),
    }
}

fn guess_categories(name: &str, capabilities: &[String]) -> Vec<String> {
    let lower = name.to_lowercase();
    let has = |cap: &str| capabilities.iter().any(|c| c == cap);
    let mut categories = Vec::new();

    let coding = ["coder", "code", "devstral", "starcoder", "codestral"]
        .iter()
        .any(|w| lower.contains(w));
    let vision = has("vision") || lower.contains("-vl") || lower.contains("vl-") || lower.contains("vision");
    let reasoning = has("thinking")
        || ["-r1", "reason", "qwq", "think", "math"]
            .iter()
            .any(|w| lower.contains(w));
    let embedding = has("embedding") || lower.contains("embed");

    if !coding && !embedding {
        categories.push("everyday".to_string());
    }
    if coding {
        categories.push("coding".to_string());
    }
    if reasoning {
        categories.push("reasoning".to_string());
    }
    if vision {
        categories.push("vision".to_string());
    }
    if vision && has("tools") && has("thinking") {
        categories.push("all-in-one".to_string());
    }
    categories
}

pub fn update(ui: &AppWindow, installed: &[InstalledModel]) {
    let catalog = catalog::load();
    let hardware = hardware::get();

    let installed_entries: Vec<InstalledEntry> = installed
        .iter()
        .map(|model| {
            let fit = hardware.estimate(model.size);
            let known = catalog::find(&catalog, &model.name);
            let test = storage::load_model_test(&model.name);
            let subtitle = match &test {
                Some(test) => trf(
                    "{}  ·  {} tok/s measured",
                    &[&format_size(model.size), &format!("{:.0}", test.tokens_per_sec)],
                ),
                None => format!("{}  ·  {}", format_size(model.size), fit.fit_text()),
            };
            InstalledEntry {
                name: model.name.as_str().into(),
                details: model.details_text().into(),
                subtitle: subtitle.into(),
                size_text: format_size(model.size).into(),
                fit_text: fit.fit_text().into(),
                vram_fill: fit.vram_fill,
                ram_fill: fit.ram_fill,
                vram_label: fit.vram_label.as_str().into(),
                ram_label: fit.ram_label.as_str().into(),
                speed: tr(fit.speed_text()).into(),
                speed_level: fit.level,
                test_text: test.as_ref().map(test_text).unwrap_or_default().into(),
                description: known.map(|m| m.description.as_str()).unwrap_or("").into(),
                strengths: string_model(known.map(|m| m.strengths.as_slice()).unwrap_or(&[])),
                weaknesses: string_model(known.map(|m| m.weaknesses.as_slice()).unwrap_or(&[])),
            }
        })
        .collect();

    ui.set_installed_models(ModelRc::new(Rc::new(VecModel::from(installed_entries))));
    STATE.with(|s| s.borrow_mut().installed = installed.iter().map(|m| m.name.clone()).collect());
    render(ui);
}

fn test_text(test: &TestResult) -> String {
    let total = test.vram + test.ram;
    let percent = if total > 0 {
        (test.vram as f64 / total as f64 * 100.0).round()
    } else {
        0.0
    };
    trf(
        "{}% VRAM ({})  ·  {} RAM  ·  {} tok/s  ·  loaded in {} s",
        &[
            &format!("{percent:.0}"),
            &format_gb(test.vram),
            &format_gb(test.ram),
            &format!("{:.1}", test.tokens_per_sec),
            &format!("{:.1}", test.load_secs),
        ],
    )
}

fn render(ui: &AppWindow) {
    STATE.with(|s| {
        let state = s.borrow();
        let items: Vec<&Item> = visible_items(&state);

        let mut groups: Vec<(String, Vec<&Item>)> = Vec::new();
        for item in &items {
            match groups.iter_mut().find(|(g, _)| *g == item.group) {
                Some((_, list)) => list.push(item),
                None => groups.push((item.group.clone(), vec![item])),
            }
        }

        let selected = state.selected.clone().unwrap_or_default();
        let mut rows: Vec<AvailableRow> = Vec::new();
        for (group, list) in &groups {
            if list.len() == 1 {
                rows.push(item_row(list[0], &selected, false));
                continue;
            }
            let expanded = state.expanded.contains(group);
            rows.push(AvailableRow {
                is_header: true,
                key: group.as_str().into(),
                title: group.as_str().into(),
                subtitle: trf("{} models", &[&list.len()]).into(),
                expanded,
                active: false,
                indented: false,
            });
            if expanded {
                rows.extend(list.iter().map(|item| item_row(item, &selected, true)));
            }
        }

        let message = if state.loading {
            tr("Searching…")
        } else if !state.message.is_empty() {
            state.message.clone()
        } else if rows.is_empty() {
            tr("No models found.")
        } else {
            String::new()
        };

        ui.set_available_rows(ModelRc::new(Rc::new(VecModel::from(rows))));
        ui.set_available_message(message.into());

        let detail = state
            .selected
            .as_ref()
            .and_then(|key| all_items(&state).find(|item| &item.key == key));
        match detail {
            Some(item) => {
                ui.set_available_detail(detail_entry(item, &state.installed));
                ui.set_has_available_detail(true);
            }
            None => ui.set_has_available_detail(false),
        }
    });
}

fn all_items(state: &State) -> impl Iterator<Item = &Item> {
    state.curated.iter().chain(state.online.iter())
}

fn visible_items(state: &State) -> Vec<&Item> {
    let query = state.query.to_lowercase();
    let category = CATEGORIES[state.category.min(CATEGORIES.len() - 1)];
    let list = match state.source {
        Source::Curated => &state.curated,
        _ => &state.online,
    };
    list.iter()
        .filter(|item| {
            state.source != Source::Curated
                || query.is_empty()
                || item.id.to_lowercase().contains(&query)
                || item.group.to_lowercase().contains(&query)
                || item.description.to_lowercase().contains(&query)
        })
        .filter(|item| category.is_empty() || item.categories.iter().any(|c| c == category))
        .collect()
}

fn item_row(item: &Item, selected: &str, indented: bool) -> AvailableRow {
    let mut subtitle = if item.capabilities.is_empty() {
        tr("text only")
    } else {
        item.capabilities.join("  ·  ")
    };
    if !item.downloads.is_empty() {
        subtitle = format!("{subtitle}  ·  ↓ {}", item.downloads);
    }
    AvailableRow {
        is_header: false,
        key: item.key.as_str().into(),
        title: item.id.as_str().into(),
        subtitle: subtitle.into(),
        expanded: false,
        active: item.key == selected,
        indented,
    }
}

fn detail_entry(item: &Item, installed: &[String]) -> CatalogEntry {
    let hardware = hardware::get();
    let variants: Vec<ModelVariant> = item
        .variants
        .iter()
        .map(|variant| {
            let fit = hardware.estimate(variant.size);
            let prefix = if variant.estimated { "≈ " } else { "" };
            ModelVariant {
                tag: variant.tag.as_str().into(),
                label: variant.label.as_str().into(),
                size_text: format!("{prefix}{}", format_size(variant.size)).into(),
                vram_fill: fit.vram_fill,
                ram_fill: fit.ram_fill,
                vram_label: fit.vram_label.as_str().into(),
                ram_label: fit.ram_label.as_str().into(),
                fit_text: fit.fit_text().into(),
                speed: tr(fit.speed_text()).into(),
                speed_level: fit.level,
                installed: installed.iter().any(|name| {
                    *name == variant.tag || *name == format!("{}:latest", variant.tag)
                }),
            }
        })
        .collect();

    let note = if let Some(error) = &item.variants_error {
        trf("Could not load sizes: {}", &[&error])
    } else if !item.variants_loaded {
        tr("Loading sizes…")
    } else if item.variants.is_empty() {
        tr("No local download found (maybe cloud only).")
    } else if item.variants.iter().any(|v| v.estimated) {
        tr("≈ Sizes estimated from the parameter count (Q4).")
    } else {
        String::new()
    };

    let source = if item.downloads.is_empty() {
        tr(item.source.label())
    } else {
        trf("{}  ·  {} downloads", &[&tr(item.source.label()), &item.downloads])
    };

    CatalogEntry {
        name: item.id.as_str().into(),
        description: item.description.as_str().into(),
        capabilities: item.capabilities.join("  ·  ").into(),
        categories: item.categories.join("  ·  ").into(),
        source: source.into(),
        note: note.into(),
        strengths: string_model(&item.strengths),
        weaknesses: string_model(&item.weaknesses),
        variants: ModelRc::new(Rc::new(VecModel::from(variants))),
    }
}

fn string_model(items: &[String]) -> ModelRc<SharedString> {
    let list: Vec<SharedString> = items.iter().map(SharedString::from).collect();
    ModelRc::new(Rc::new(VecModel::from(list)))
}

fn search(ui: &AppWindow) {
    let (source, query, generation) = STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.generation += 1;
        if !state.query.is_empty() {
            let groups: Vec<String> = match state.source {
                Source::Curated => state.curated.iter().map(|i| i.group.clone()).collect(),
                _ => Vec::new(),
            };
            state.expanded.extend(groups);
        }
        if state.source != Source::Curated {
            state.loading = true;
            state.message.clear();
        }
        (state.source, state.query.clone(), state.generation)
    });
    render(ui);
    if source == Source::Curated {
        return;
    }

    let weak = ui.as_weak();
    thread::spawn(move || {
        let result = match source {
            Source::Ollama => online::search_ollama(&query),
            _ => online::search_hf(&query),
        };
        let _ = weak.upgrade_in_event_loop(move |ui| {
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                if state.generation != generation {
                    return;
                }
                state.loading = false;
                match result {
                    Ok(models) => {
                        let mut items: Vec<Item> =
                            models.into_iter().map(|m| online_item(source, m)).collect();
                        for item in &mut items {
                            if let Some(old) = state
                                .online
                                .iter()
                                .find(|old| old.key == item.key && old.variants_loaded)
                            {
                                item.variants = old.variants.clone();
                                item.variants_loaded = true;
                            }
                        }
                        let selected_still_there = state
                            .selected
                            .as_ref()
                            .is_some_and(|key| items.iter().any(|item| &item.key == key));
                        if !selected_still_there && state.source != Source::Curated {
                            state.selected = None;
                        }
                        state.online = items;
                        if !query.is_empty() {
                            let groups: Vec<String> =
                                state.online.iter().map(|i| i.group.clone()).collect();
                            state.expanded.extend(groups);
                        }
                    }
                    Err(e) => {
                        state.online.clear();
                        state.message = trf("Search failed: {}", &[&e]);
                    }
                }
            });
            render(&ui);
        });
    });
}

fn select(ui: &AppWindow, key: String) {
    let fetch = STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.selected = Some(key.clone());
        state
            .online
            .iter()
            .find(|item| item.key == key && !item.variants_loaded && item.variants_error.is_none())
            .map(|item| (item.source, item.id.clone()))
    });
    render(ui);

    let Some((source, id)) = fetch else { return };
    let weak = ui.as_weak();
    thread::spawn(move || {
        let result = match source {
            Source::Ollama => online::ollama_variants(&id),
            _ => online::hf_variants(&id),
        };
        let _ = weak.upgrade_in_event_loop(move |ui| {
            STATE.with(|s| {
                let mut state = s.borrow_mut();
                let Some(item) = state.online.iter_mut().find(|item| item.key == key) else {
                    return;
                };
                match result {
                    Ok(variants) => {
                        if !variants.is_empty() || item.variants.is_empty() {
                            item.variants = variants.into_iter().map(variant_info).collect();
                        }
                        item.variants_loaded = true;
                    }
                    Err(e) => item.variants_error = Some(e),
                }
            });
            render(&ui);
        });
    });
}

fn variant_info(variant: OnlineVariant) -> VariantInfo {
    VariantInfo {
        tag: variant.tag,
        label: variant.label,
        size: variant.size,
        estimated: false,
    }
}

fn install(ui: &AppWindow, tag: &str) {
    let valid = !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | ':' | '/' | '-'));
    if !valid {
        ui.set_status(trf("Invalid model name: {}", &[&tag]).into());
        return;
    }

    let script = format!("ollama pull {tag}; echo; echo 'Done. Press Enter to close.'; read _");
    match crate::terminal::run(&script) {
        Ok(mut child) => {
            let weak = ui.as_weak();
            thread::spawn(move || {
                let _ = child.wait();
                let _ = weak.upgrade_in_event_loop(|ui| ui.invoke_refresh_models());
            });
        }
        Err(e) => ui.set_status(e.into()),
    }
}

fn remove(ui: &AppWindow, name: String) {
    if ui.get_busy() {
        return;
    }
    ui.set_status(trf("Removing {}…", &[&name]).into());
    let weak = ui.as_weak();
    thread::spawn(move || {
        let result = ollama::delete_model(&name);
        let _ = weak.upgrade_in_event_loop(move |ui| {
            match result {
                Ok(()) => ui.set_status(trf("{} removed", &[&name]).into()),
                Err(e) => ui.set_status(trf("Could not remove {}: {}", &[&name, &e]).into()),
            }
            ui.invoke_refresh_models();
        });
    });
}

fn test(ui: &AppWindow, name: String) {
    if !ui.get_testing_model().is_empty() || ui.get_busy() {
        return;
    }
    ui.set_testing_model(name.as_str().into());
    let weak = ui.as_weak();
    thread::spawn(move || {
        let result = ollama::test_model(&name);
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.set_testing_model("".into());
            match result {
                Ok(test) => {
                    if let Err(e) = storage::save_model_test(&name, &test) {
                        ui.set_status(trf("Could not save test result: {}", &[&e]).into());
                    }
                    ui.invoke_refresh_models();
                }
                Err(e) => ui.set_status(trf("Test of {} failed: {}", &[&name, &e]).into()),
            }
        });
    });
}
