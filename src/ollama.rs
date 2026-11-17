use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::sync::{Mutex, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};

static BASE_URL: RwLock<String> = RwLock::new(String::new());

pub fn set_base_url(url: &str) {
    let url = url.trim().trim_end_matches('/');
    let url = if url.is_empty() { "http://localhost:11434" } else { url };
    if let Ok(mut base) = BASE_URL.write() {
        *base = url.to_string();
    }
}

fn base_url() -> String {
    BASE_URL
        .read()
        .ok()
        .filter(|url| !url.is_empty())
        .map(|url| url.clone())
        .unwrap_or_else(|| "http://localhost:11434".to_string())
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<String>,
    #[serde(skip)]
    pub image_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub function: ToolFunction,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ToolFunction {
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

pub struct ChatOutcome {
    pub stats: Option<ChatStats>,
    pub tool_calls: Vec<ToolCall>,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into()
}

fn check_status(response: &mut ureq::http::Response<ureq::Body>) -> Result<(), String> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let body = response.body_mut().read_to_string().unwrap_or_default();
    let message = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| value["error"].as_str().map(str::to_string))
        .unwrap_or_else(|| body.trim().to_string());
    if message.is_empty() {
        Err(format!("HTTP {status}"))
    } else {
        Err(format!("HTTP {}: {message}", status.as_u16()))
    }
}

pub fn unload(model: &str) {
    let request = GenerateRequest {
        model,
        prompt: "",
        stream: false,
        keep_alive: Some(0),
        options: None,
    };
    let _ = ureq::post(format!("{}/api/generate", base_url())).send_json(&request);
}

pub fn unload_others(model: &str) -> usize {
    let others: Vec<String> = running_models()
        .unwrap_or_default()
        .into_iter()
        .map(|m| m.name)
        .filter(|name| name != model)
        .collect();
    for name in &others {
        unload(name);
    }
    others.len()
}

pub fn is_out_of_memory(error: &str) -> bool {
    let error = error.to_lowercase();
    ["out of memory", "failed to allocate", "cudamalloc failed", "insufficient memory"]
        .iter()
        .any(|text| error.contains(text))
}

pub fn is_runner_crash(error: &str) -> bool {
    let error = error.to_lowercase();
    error.contains("process has terminated")
        && ["segmentation fault", "signal", "core dumped", "exit status"]
            .iter()
            .any(|text| error.contains(text))
}

pub const CRASH_HELP: &str = "Ollama's model runner crashed – this is a problem inside Ollama, not in this app.\n\n- Restart Ollama (button bottom left: Stop, then Start) and try again.\n- AMD graphics card: the ROCm version of Ollama often crashes on RX 9000 cards. Try the Vulkan version instead (CachyOS/Arch: install `ollama-vulkan` instead of `ollama-rocm`) or update Ollama.\n- Try a smaller context in the Ollama settings or another model.\n- Details: `journalctl -u ollama -n 50` in a terminal.";

pub fn is_running() -> bool {
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(2)))
        .build()
        .new_agent()
        .get(format!("{}/api/version", base_url()))
        .call()
        .is_ok()
}

static CAPABILITIES: Mutex<Vec<(String, Vec<String>)>> = Mutex::new(Vec::new());

pub fn supports_vision(model: &str) -> bool {
    capabilities(model).iter().any(|c| c == "vision")
}

pub fn supports_thinking(model: &str) -> bool {
    capabilities(model).iter().any(|c| c == "thinking")
}

pub fn supports_tools(model: &str) -> bool {
    capabilities(model).iter().any(|c| c == "tools")
}

fn capabilities(model: &str) -> Vec<String> {
    if let Ok(cache) = CAPABILITIES.lock()
        && let Some((_, caps)) = cache.iter().find(|(name, _)| name == model)
    {
        return caps.clone();
    }
    let caps: Vec<String> = agent()
        .post(format!("{}/api/show", base_url()))
        .send_json(serde_json::json!({ "model": model }))
        .ok()
        .and_then(|mut response| {
            check_status(&mut response).ok()?;
            response.body_mut().read_json::<serde_json::Value>().ok()
        })
        .and_then(|value| {
            value["capabilities"].as_array().map(|list| {
                list.iter()
                    .filter_map(|c| c.as_str().map(str::to_string))
                    .collect()
            })
        })
        .unwrap_or_else(|| vec!["vision".to_string()]);
    if let Ok(mut cache) = CAPABILITIES.lock() {
        cache.push((model.to_string(), caps.clone()));
    }
    caps
}

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<ModelInfo>,
}

#[derive(Deserialize)]
struct ModelInfo {
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    details: ModelDetails,
}

#[derive(Deserialize, Default)]
struct ModelDetails {
    #[serde(default)]
    parameter_size: String,
    #[serde(default)]
    quantization_level: String,
}

pub struct InstalledModel {
    pub name: String,
    pub size: u64,
    pub parameter_size: String,
    pub quantization: String,
}

impl InstalledModel {
    pub fn details_text(&self) -> String {
        [self.parameter_size.as_str(), self.quantization.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("  ·  ")
    }
}

#[derive(Deserialize)]
struct PsResponse {
    models: Vec<RunningModel>,
}

#[derive(Deserialize)]
pub struct RunningModel {
    pub name: String,
    pub size: u64,
    #[serde(default)]
    pub size_vram: u64,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    stream: bool,
    keep_alive: KeepAlive,
    options: ChatOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    think: Option<bool>,
}

#[derive(Serialize)]
#[serde(untagged)]
enum KeepAlive {
    Seconds(i64),
    Text(String),
}

fn keep_alive(value: &str) -> KeepAlive {
    match value.parse::<i64>() {
        Ok(seconds) => KeepAlive::Seconds(seconds),
        Err(_) => KeepAlive::Text(value.to_string()),
    }
}

#[derive(Serialize)]
struct ChatOptions {
    num_ctx: u32,
}

pub struct ChatConfig {
    pub context_length: u32,
    pub keep_alive: String,
    pub think: Option<bool>,
}

pub struct ChatStats {
    pub prompt_tokens: u64,
    pub output_tokens: u64,
}

pub fn estimate_tokens(messages: &[Message]) -> u64 {
    let chars: usize = messages.iter().map(|m| m.content.chars().count()).sum();
    let images: usize = messages.iter().map(|m| m.images.len()).sum();
    (chars as u64) * 10 / 35 + images as u64 * 800 + messages.len() as u64 * 8
}

#[derive(Deserialize)]
struct ChatChunk {
    message: Option<ChunkMessage>,
    #[serde(default)]
    done: bool,
    error: Option<String>,
    #[serde(default)]
    prompt_eval_count: u64,
    #[serde(default)]
    eval_count: u64,
}

#[derive(Deserialize)]
struct ChunkMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    thinking: String,
    #[serde(default)]
    tool_calls: Vec<ToolCall>,
}

pub fn list_models() -> Result<Vec<InstalledModel>, String> {
    let mut response = ureq::get(format!("{}/api/tags", base_url()))
        .call()
        .map_err(|e| e.to_string())?;
    let tags: TagsResponse = response
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    let mut models: Vec<InstalledModel> = tags
        .models
        .into_iter()
        .map(|m| InstalledModel {
            name: m.name,
            size: m.size,
            parameter_size: m.details.parameter_size,
            quantization: m.details.quantization_level,
        })
        .collect();
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

pub fn running_models() -> Result<Vec<RunningModel>, String> {
    let mut response = ureq::get(format!("{}/api/ps", base_url()))
        .call()
        .map_err(|e| e.to_string())?;
    let ps: PsResponse = response
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    Ok(ps.models)
}

pub fn chat_stream(
    model: &str,
    messages: &[Message],
    config: &ChatConfig,
    tools: Option<&serde_json::Value>,
    cancel: &AtomicBool,
    mut on_chunk: impl FnMut(&str, bool),
) -> Result<ChatOutcome, String> {
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let request = ChatRequest {
        model,
        messages,
        stream: true,
        keep_alive: keep_alive(&config.keep_alive),
        options: ChatOptions {
            num_ctx: config.context_length,
        },
        tools,
        think: config.think,
    };
    let mut response = agent()
        .post(format!("{}/api/chat", base_url()))
        .send_json(&request)
        .map_err(|e| e.to_string())?;
    check_status(&mut response)?;

    let reader = BufReader::new(response.into_body().into_reader());
    for line in reader.lines() {
        if cancel.load(Ordering::SeqCst) {
            return Ok(ChatOutcome {
                stats: None,
                tool_calls: Vec::new(),
            });
        }
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let chunk: ChatChunk = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if let Some(error) = chunk.error {
            return Err(error);
        }
        if let Some(message) = chunk.message {
            if !message.thinking.is_empty() {
                on_chunk(&message.thinking, true);
            }
            if !message.content.is_empty() {
                on_chunk(&message.content, false);
            }
            tool_calls.extend(message.tool_calls);
        }
        if chunk.done {
            return Ok(ChatOutcome {
                stats: Some(ChatStats {
                    prompt_tokens: chunk.prompt_eval_count,
                    output_tokens: chunk.eval_count,
                }),
                tool_calls,
            });
        }
    }
    Ok(ChatOutcome {
        stats: None,
        tool_calls,
    })
}

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    keep_alive: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<GenerateOptions>,
}

#[derive(Serialize)]
struct GenerateOptions {
    num_predict: i32,
    temperature: f32,
}

#[derive(Deserialize)]
struct GenerateResponse {
    #[serde(default)]
    eval_count: u64,
    #[serde(default)]
    eval_duration: u64,
    #[serde(default)]
    load_duration: u64,
    error: Option<String>,
}

#[derive(Serialize)]
struct ClassifyRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
    keep_alive: KeepAlive,
    think: bool,
    options: GenerateOptions,
}

#[derive(Deserialize)]
struct ClassifyResponse {
    #[serde(default)]
    response: String,
    error: Option<String>,
}

#[derive(Deserialize)]
struct EmbedResponse {
    #[serde(default)]
    embeddings: Vec<Vec<f32>>,
    error: Option<String>,
}

pub fn embed(model: &str, inputs: &[String]) -> Result<Vec<Vec<f32>>, String> {
    let body = serde_json::json!({ "model": model, "input": inputs, "keep_alive": "10m" });
    let mut response = agent()
        .post(format!("{}/api/embed", base_url()))
        .send_json(&body)
        .map_err(|e| e.to_string())?;
    let result: EmbedResponse = response.body_mut().read_json().map_err(|e| e.to_string())?;
    if let Some(error) = result.error {
        return Err(error);
    }
    if result.embeddings.len() != inputs.len() {
        return Err("the embedding model gave a wrong number of results".to_string());
    }
    Ok(result.embeddings)
}

pub fn classify(model: &str, prompt: &str, keep: &str) -> Result<String, String> {
    classify_with(model, prompt, keep, 8)
}

pub fn classify_with(model: &str, prompt: &str, keep: &str, tokens: i32) -> Result<String, String> {
    let request = ClassifyRequest {
        model,
        prompt,
        stream: false,
        keep_alive: keep_alive(keep),
        think: false,
        options: GenerateOptions {
            num_predict: tokens,
            temperature: 0.0,
        },
    };
    let mut response = agent()
        .post(format!("{}/api/generate", base_url()))
        .send_json(&request)
        .map_err(|e| e.to_string())?;
    let result: ClassifyResponse = response.body_mut().read_json().map_err(|e| e.to_string())?;
    match result.error {
        Some(error) => Err(error),
        None => Ok(result.response),
    }
}

pub struct TestResult {
    pub vram: u64,
    pub ram: u64,
    pub tokens_per_sec: f64,
    pub load_secs: f64,
}

pub fn test_model(model: &str) -> Result<TestResult, String> {
    let was_loaded = running_models()
        .map(|models| models.iter().any(|m| m.name == model))
        .unwrap_or(false);

    let request = GenerateRequest {
        model,
        prompt: "Write three short sentences about the ocean.",
        stream: false,
        keep_alive: None,
        options: Some(GenerateOptions {
            num_predict: 128,
            temperature: 0.0,
        }),
    };
    let mut response = ureq::post(format!("{}/api/generate", base_url()))
        .send_json(&request)
        .map_err(|e| e.to_string())?;
    let result: GenerateResponse = response
        .body_mut()
        .read_json()
        .map_err(|e| e.to_string())?;
    if let Some(error) = result.error {
        return Err(error);
    }

    let running = running_models()?;
    let loaded = running
        .iter()
        .find(|m| m.name == model)
        .ok_or_else(|| crate::i18n::tr("Model was not loaded after the test"))?;
    let vram = loaded.size_vram;
    let ram = loaded.size.saturating_sub(loaded.size_vram);

    if !was_loaded {
        unload(model);
    }

    let tokens_per_sec = if result.eval_duration > 0 {
        result.eval_count as f64 / (result.eval_duration as f64 / 1e9)
    } else {
        0.0
    };
    Ok(TestResult {
        vram,
        ram,
        tokens_per_sec,
        load_secs: result.load_duration as f64 / 1e9,
    })
}
