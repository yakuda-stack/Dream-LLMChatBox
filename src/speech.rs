use crate::i18n::{tr, trf};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main";

pub const MODELS: [(&str, &str, &str); 3] = [
    ("base", "ggml-base.bin", "Base – fast (~150 MB)"),
    ("small", "ggml-small.bin", "Small – better (~490 MB)"),
    ("turbo", "ggml-large-v3-turbo-q5_0.bin", "Turbo – best (~570 MB)"),
];

pub const LANGUAGES: [(&str, &str); 12] = [
    ("auto", "Automatic"),
    ("de", "Deutsch"),
    ("en", "English"),
    ("fr", "Français"),
    ("es", "Español"),
    ("it", "Italiano"),
    ("nl", "Nederlands"),
    ("pl", "Polski"),
    ("pt", "Português"),
    ("ru", "Русский"),
    ("tr", "Türkçe"),
    ("ja", "日本語"),
];

fn models_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dream-llmchatbox")
        .join("whisper")
}

fn model_file(model: &str) -> &'static str {
    MODELS
        .iter()
        .find(|(id, _, _)| *id == model)
        .map(|(_, file, _)| *file)
        .unwrap_or(MODELS[0].1)
}

pub fn model_path(model: &str) -> PathBuf {
    models_dir().join(model_file(model))
}

pub fn ensure_model(model: &str, mut progress: impl FnMut(u32)) -> Result<PathBuf, String> {
    let path = model_path(model);
    if path.exists() {
        return Ok(path);
    }
    fs::create_dir_all(models_dir()).map_err(|e| e.to_string())?;

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .build()
        .into();
    let response = agent
        .get(format!("{MODEL_URL}/{}", model_file(model)))
        .call()
        .map_err(|e| trf("Download failed: {}", &[&e]))?;
    let total: u64 = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let part = path.with_extension("part");
    let mut file = fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut reader = response.into_body().into_reader();
    let mut buffer = vec![0u8; 1 << 16];
    let mut done: u64 = 0;
    let mut last_percent = u32::MAX;
    loop {
        let read = reader.read(&mut buffer).map_err(|e| trf("Download failed: {}", &[&e]))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
        done += read as u64;
        if let Some(percent) = (done * 100).checked_div(total).map(|p| p as u32)
            && percent != last_percent
        {
            last_percent = percent;
            progress(percent);
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    if total > 0 && done != total {
        return Err(tr("Download incomplete"));
    }
    fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn transcribe(model: &PathBuf, samples: &[f32], language: &str) -> Result<String, String> {
    static CONTEXT: OnceLock<Mutex<Option<(PathBuf, WhisperContext)>>> = OnceLock::new();
    static HOOKS: OnceLock<()> = OnceLock::new();
    HOOKS.get_or_init(whisper_rs::install_logging_hooks);

    if samples.len() < 16_000 / 2 {
        return Err(tr("Recording too short"));
    }

    let lock = CONTEXT.get_or_init(|| Mutex::new(None));
    let mut slot = lock.lock().map_err(|_| tr("Speech engine busy"))?;
    if slot.as_ref().is_none_or(|(path, _)| path != model) {
        let context = WhisperContext::new_with_params(model, WhisperContextParameters::default())
            .map_err(|e| trf("Could not load speech model: {}", &[&e]))?;
        *slot = Some((model.clone(), context));
    }
    let (_, context) = slot.as_ref().ok_or(tr("Speech model missing"))?;

    let mut state = context.create_state().map_err(|e| e.to_string())?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(language));
    params.set_n_threads(
        std::thread::available_parallelism()
            .map(|n| n.get().min(8) as i32)
            .unwrap_or(4),
    );
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_no_context(true);

    state.full(params, samples).map_err(|e| e.to_string())?;
    let text: Vec<String> = state
        .as_iter()
        .filter_map(|segment| segment.to_str_lossy().ok().map(|s| s.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect();
    Ok(text.join(" "))
}
