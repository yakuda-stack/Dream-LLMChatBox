use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub last_model: String,
    #[serde(default)]
    pub mic_device: String,
    #[serde(default = "default_language")]
    pub speech_language: String,
    #[serde(default = "default_gain")]
    pub mic_gain: f32,
    #[serde(default = "default_true")]
    pub live_speech: bool,
    #[serde(default = "default_speech_model")]
    pub speech_model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_about: String,
    #[serde(default)]
    pub active_profile: String,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    #[serde(default = "default_context")]
    pub context_length: u32,
    #[serde(default = "default_keep_alive")]
    pub keep_alive: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub tts_voice: String,
    #[serde(default)]
    pub answer_language: String,
    #[serde(default = "default_true")]
    pub tools_enabled: bool,
    #[serde(default)]
    pub auto_switch: bool,
    #[serde(default)]
    pub cloud_active: Vec<String>,
    #[serde(default)]
    pub servers: Vec<crate::openai_compat::Server>,
    #[serde(default)]
    pub embed_model: String,
    #[serde(default = "default_true")]
    pub auto_continue: bool,
    #[serde(default = "default_true")]
    pub show_context_bar: bool,
    #[serde(default = "default_true")]
    pub auto_compact: bool,
    #[serde(default)]
    pub welcome_seen: bool,
    #[serde(default)]
    pub ui_language: String,
    #[serde(default)]
    pub ollama_backend: u32,
    #[serde(default)]
    pub theme: u32,
    #[serde(default)]
    pub container_engine: String,
    #[serde(default)]
    pub container_image: String,
    #[serde(default = "default_compact_at")]
    pub compact_at: u32,
    #[serde(default)]
    pub work_dir: String,
    #[serde(default)]
    pub model_context: BTreeMap<String, u32>,
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    #[serde(default)]
    pub tool_descriptions: BTreeMap<String, String>,
    #[serde(default = "default_safe_commands")]
    pub safe_commands: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            last_model: String::new(),
            mic_device: String::new(),
            speech_language: default_language(),
            mic_gain: default_gain(),
            live_speech: true,
            speech_model: default_speech_model(),
            user_name: String::new(),
            user_about: String::new(),
            active_profile: String::new(),
            ollama_url: default_ollama_url(),
            context_length: default_context(),
            keep_alive: default_keep_alive(),
            system_prompt: String::new(),
            tts_voice: String::new(),
            answer_language: String::new(),
            tools_enabled: true,
            auto_switch: false,
            cloud_active: Vec::new(),
            servers: Vec::new(),
            embed_model: String::new(),
            auto_continue: true,
            show_context_bar: true,
            auto_compact: true,
            welcome_seen: false,
            ui_language: String::new(),
            ollama_backend: 0,
            theme: 0,
            container_engine: String::new(),
            container_image: String::new(),
            compact_at: 80,
            work_dir: String::new(),
            model_context: BTreeMap::new(),
            disabled_tools: Vec::new(),
            tool_descriptions: BTreeMap::new(),
            safe_commands: default_safe_commands(),
        }
    }
}

impl Settings {
    pub fn context_for(&self, model: &str) -> u32 {
        let wanted = self.context_length.max(2048);
        self.model_context
            .get(model)
            .map_or(wanted, |&fits| fits.min(wanted))
    }
}

pub fn default_safe_commands() -> Vec<String> {
    crate::commands::DEFAULT_SAFE.iter().map(|s| s.to_string()).collect()
}

fn default_compact_at() -> u32 {
    80
}

fn default_true() -> bool {
    true
}

fn default_language() -> String {
    "auto".to_string()
}

fn default_gain() -> f32 {
    1.0
}

fn default_speech_model() -> String {
    "base".to_string()
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_context() -> u32 {
    65536
}

fn default_keep_alive() -> String {
    "5m".to_string()
}

fn settings_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dream-llmchatbox")
        .join("settings.json")
}

pub fn load() -> Settings {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

pub fn save(settings: &Settings) -> Result<(), String> {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

pub fn update(change: impl FnOnce(&mut Settings)) {
    let mut settings = load();
    change(&mut settings);
    let _ = save(&settings);
}
