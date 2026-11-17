use crate::AppWindow;
use crate::ollama;
use slint::{ComponentHandle, Timer, TimerMode};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

pub fn start(ui: &AppWindow) -> Timer {
    let timer = Timer::default();
    let weak = ui.as_weak();
    let polling = Arc::new(AtomicBool::new(false));

    timer.start(TimerMode::Repeated, Duration::from_secs(1), move || {
        let Some(ui) = weak.upgrade() else { return };
        if !ui.get_busy() {
            ui.set_usage_text("".into());
            return;
        }
        if polling.swap(true, Ordering::SeqCst) {
            return;
        }
        let weak = weak.clone();
        let polling = polling.clone();
        thread::spawn(move || {
            let result = ollama::running_models();
            polling.store(false, Ordering::SeqCst);
            let _ = weak.upgrade_in_event_loop(move |ui| match result {
                Ok(models) if ui.get_busy() && !models.is_empty() => {
                    let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
                    let vram: u64 = models.iter().map(|m| m.size_vram).sum();
                    let total: u64 = models.iter().map(|m| m.size).sum();
                    let ram = total.saturating_sub(vram);
                    ui.set_usage_text(
                        format!(
                            "{}  ·  VRAM {}  ·  RAM {}",
                            names.join(", "),
                            format_bytes(vram),
                            format_bytes(ram)
                        )
                        .into(),
                    );
                }
                _ => ui.set_usage_text("".into()),
            });
        });
    });

    timer
}

fn format_bytes(bytes: u64) -> String {
    let gb = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", gb * 1024.0)
    }
}
