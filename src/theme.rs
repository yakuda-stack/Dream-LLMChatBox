use crate::settings;
use crate::AppWindow;

pub fn setup(ui: &AppWindow) {
    ui.set_theme_index(settings::load().theme.min(2) as i32);
    ui.on_theme_changed(|index| {
        settings::update(|s| s.theme = index.clamp(0, 2) as u32);
    });
}
