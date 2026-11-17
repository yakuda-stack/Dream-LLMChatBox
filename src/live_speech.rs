use crate::audio::Live;
use crate::speech;
use crate::AppWindow;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

const RATE: usize = 16_000;
const CHUNK: usize = 20 * RATE;
const EVERY: Duration = Duration::from_millis(1200);

static SESSION: AtomicU64 = AtomicU64::new(0);

thread_local! {
    static BASE: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub fn join(left: &str, right: &str) -> String {
    match (left.trim().is_empty(), right.trim().is_empty()) {
        (true, _) => right.trim().to_string(),
        (_, true) => left.trim_end().to_string(),
        _ => format!("{} {}", left.trim_end(), right.trim()),
    }
}

pub fn start(ui: &AppWindow, live: Live, model: PathBuf, language: String) {
    let session = SESSION.fetch_add(1, Ordering::SeqCst) + 1;
    let base = ui.get_draft().to_string();
    BASE.with(|slot| *slot.borrow_mut() = Some(base.clone()));
    let weak = ui.as_weak();
    thread::spawn(move || {
        let mut committed = String::new();
        let mut done = 0;
        loop {
            thread::sleep(EVERY);
            let current = || live.is_recording() && SESSION.load(Ordering::SeqCst) == session;
            if !current() {
                break;
            }
            let tail = live.snapshot_from(done);
            if tail.len() < RATE {
                continue;
            }
            let finished = tail.len() >= CHUNK;
            let part = if finished { &tail[..CHUNK] } else { &tail[..] };
            let Ok(text) = speech::transcribe(&model, part, &language) else { continue };
            let preview = if finished {
                committed = join(&committed, &text);
                done += CHUNK;
                committed.clone()
            } else {
                join(&committed, &text)
            };
            if !current() {
                break;
            }
            let shown = join(&base, &preview);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                if ui.get_mic_recording() && SESSION.load(Ordering::SeqCst) == session {
                    ui.set_draft(shown.into());
                }
            });
        }
    });
}

pub fn stop(ui: &AppWindow) {
    SESSION.fetch_add(1, Ordering::SeqCst);
    if let Some(base) = BASE.with(|slot| slot.borrow_mut().take()) {
        ui.set_draft(base.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_text() {
        assert_eq!(join("", " Hallo "), "Hallo");
        assert_eq!(join("Hi ", ""), "Hi");
        assert_eq!(join("Hi", "du"), "Hi du");
    }
}
