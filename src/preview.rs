use crate::i18n::{tr, trf};
use crate::checkpoint;
use crate::{AppWindow, PreviewState};
use slint::{ComponentHandle, Image, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const IMAGES: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico"];
const MAX_TEXT: u64 = 1_000_000;

thread_local! {
    static FILES: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    static PICKED: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

#[derive(Debug, PartialEq)]
pub enum Kind {
    Image,
    Pdf,
    Html,
    Text,
    None,
}

pub fn kind(path: &Path) -> Kind {
    let ext = path
        .extension()
        .map(|ext| ext.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if IMAGES.contains(&ext.as_str()) {
        Kind::Image
    } else if ext == "pdf" {
        Kind::Pdf
    } else if ext == "html" || ext == "htm" {
        Kind::Html
    } else if fs::read(path)
        .ok()
        .is_some_and(|bytes| !bytes.iter().take(8000).any(|byte| *byte == 0))
    {
        Kind::Text
    } else {
        Kind::None
    }
}

fn current(ui: &AppWindow) -> Option<PathBuf> {
    let index = ui.global::<PreviewState>().get_current_index();
    FILES.with(|files| files.borrow().get(index.max(0) as usize).cloned()).filter(|_| index >= 0)
}

fn pdf_pages(path: &Path) -> i32 {
    Command::new("pdfinfo")
        .arg(path)
        .output()
        .ok()
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .find_map(|line| line.strip_prefix("Pages:").and_then(|n| n.trim().parse().ok()))
        })
        .unwrap_or(1)
}

fn pdf_page(path: &Path, page: i32) -> Result<Image, String> {
    let out = std::env::temp_dir().join(format!("dream-preview-{}", std::process::id()));
    let _ = fs::create_dir_all(&out);
    let prefix = out.join("page");
    let status = Command::new("pdftoppm")
        .args(["-png", "-r", "90", "-singlefile", "-f", &page.to_string(), "-l", &page.to_string()])
        .arg(path)
        .arg(&prefix)
        .status()
        .map_err(|_| tr("pdftoppm is missing – install poppler to see PDF pages"))?;
    if !status.success() {
        return Err(tr("pdftoppm could not read this PDF"));
    }
    Image::load_from_path(&prefix.with_extension("png")).map_err(|_| tr("Could not load the PDF page"))
}

fn show(ui: &AppWindow, page: i32) {
    let state = ui.global::<PreviewState>();
    state.set_note("".into());
    let Some(path) = current(ui) else {
        state.set_kind("".into());
        return;
    };
    if !path.exists() {
        state.set_kind("none".into());
        state.set_note(trf("{} does not exist (any more)", &[&path.display()]).into());
        return;
    }
    match kind(&path) {
        Kind::Image => match Image::load_from_path(&path) {
            Ok(image) => {
                state.set_picture(image);
                state.set_kind("image".into());
            }
            Err(_) => {
                state.set_kind("none".into());
                state.set_note(tr("Could not load this image").into());
            }
        },
        Kind::Pdf => {
            let pages = pdf_pages(&path);
            let page = page.clamp(1, pages.max(1));
            state.set_pages(pages);
            state.set_page(page);
            match pdf_page(&path, page) {
                Ok(image) => {
                    state.set_picture(image);
                    state.set_kind("pdf".into());
                }
                Err(e) => {
                    state.set_kind("none".into());
                    state.set_note(e.into());
                }
            }
        }
        Kind::Html => {
            let html = fs::read_to_string(&path).unwrap_or_default();
            state.set_text(crate::web_tools::html_to_text(&html).into());
            state.set_kind("text".into());
            state.set_note(tr("HTML shown as text – ↗ opens the real page in your browser").into());
        }
        Kind::Text => {
            let too_big = fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_TEXT);
            let text = if too_big {
                tr("File is too big for the preview – open it with ↗")
            } else {
                fs::read_to_string(&path).unwrap_or_default()
            };
            state.set_text(text.into());
            state.set_kind("text".into());
        }
        Kind::None => state.set_kind("none".into()),
    }
}

fn set_files(ui: &AppWindow, files: Vec<PathBuf>, select: Option<usize>) {
    let names: Vec<SharedString> = files
        .iter()
        .map(|path| {
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let parent = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            format!("{name}  ·  {parent}").into()
        })
        .collect();
    let state = ui.global::<PreviewState>();
    state.set_files(ModelRc::new(VecModel::from(names)));
    let index = select.map(|index| index as i32).unwrap_or(-1);
    FILES.with(|list| *list.borrow_mut() = files);
    state.set_current_index(index);
    show(ui, 1);
}

pub fn refresh(ui: &AppWindow) {
    let chat = ui.get_current_chat_id().to_string();
    let previous = current(ui);
    let mut files: Vec<PathBuf> = checkpoint::paths(&chat).into_iter().map(PathBuf::from).collect();
    PICKED.with(|picked| {
        for path in picked.borrow().iter() {
            if !files.contains(path) {
                files.push(path.clone());
            }
        }
    });
    let newest_changed = files.first().is_some_and(|first| FILES.with(|old| old.borrow().first() != Some(first)));
    let select = if newest_changed && !files.is_empty() {
        Some(0)
    } else {
        previous
            .and_then(|previous| files.iter().position(|path| *path == previous))
            .or(if files.is_empty() { None } else { Some(0) })
    };
    set_files(ui, files, select);
}

pub fn chat_changed(ui: &AppWindow) {
    PICKED.with(|picked| picked.borrow_mut().clear());
    FILES.with(|files| files.borrow_mut().clear());
    refresh(ui);
}

pub fn setup(ui: &AppWindow) {
    let state = ui.global::<PreviewState>();

    let weak = ui.as_weak();
    state.on_select(move |index| {
        if let Some(ui) = weak.upgrade() {
            ui.global::<PreviewState>().set_current_index(index);
            show(&ui, 1);
        }
    });

    let weak = ui.as_weak();
    state.on_refresh(move || {
        if let Some(ui) = weak.upgrade() {
            let page = ui.global::<PreviewState>().get_page();
            show(&ui, page);
        }
    });

    let weak = ui.as_weak();
    state.on_page_step(move |step| {
        if let Some(ui) = weak.upgrade() {
            let page = ui.global::<PreviewState>().get_page() + step;
            show(&ui, page);
        }
    });

    let weak = ui.as_weak();
    state.on_open_external(move || {
        if let Some(ui) = weak.upgrade()
            && let Some(path) = current(&ui)
        {
            let _ = Command::new("xdg-open")
                .arg(path)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    });

    let weak = ui.as_weak();
    state.on_pick_file(move || {
        let Some(ui) = weak.upgrade() else { return };
        let Some(path) = rfd::FileDialog::new().set_title(tr("Preview a file")).pick_file() else { return };
        PICKED.with(|picked| {
            let mut picked = picked.borrow_mut();
            picked.retain(|p| *p != path);
            picked.insert(0, path.clone());
        });
        let mut files: Vec<PathBuf> = FILES.with(|files| files.borrow().clone());
        files.retain(|p| *p != path);
        files.insert(0, path);
        set_files(&ui, files, Some(0));
        ui.set_side_tab(2);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_extension_and_content() {
        let dir = std::env::temp_dir().join(format!("dlc-preview-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.md"), "# hi").unwrap();
        fs::write(dir.join("b.bin"), [0u8, 1, 2]).unwrap();
        assert_eq!(kind(Path::new("x.PNG")), Kind::Image);
        assert_eq!(kind(Path::new("x.pdf")), Kind::Pdf);
        assert_eq!(kind(Path::new("index.html")), Kind::Html);
        assert_eq!(kind(&dir.join("a.md")), Kind::Text);
        assert_eq!(kind(&dir.join("b.bin")), Kind::None);
        let _ = fs::remove_dir_all(&dir);
    }
}
