use crate::ollama::{ChatOutcome, ChatStats, Message, ToolCall, ToolFunction};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const MARK: &str = "⇄ ";

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Server {
    pub name: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_key: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "yes")]
    pub tools: bool,
}

pub const PRESETS: [(&str, &str); 4] = [
    ("LM Studio", "http://localhost:1234/v1"),
    ("llama.cpp", "http://localhost:8080/v1"),
    ("vLLM", "http://localhost:8000/v1"),
    ("LocalAI", "http://localhost:8080/v1"),
];

pub fn base(server: &Server) -> String {
    server.url.trim().trim_end_matches('/').to_string()
}

pub fn model_name(server: &Server, id: &str) -> String {
    format!("{MARK}{}: {id}", server.name)
}

pub fn from_model(model: &str, servers: &[Server]) -> Option<(Server, String)> {
    let rest = model.strip_prefix(MARK)?;
    servers.iter().find_map(|server| {
        let id = rest.strip_prefix(&server.name)?.strip_prefix(": ")?;
        Some((server.clone(), id.to_string()))
    })
}

fn agent(timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(timeout)
        .build()
        .into()
}

fn with_key<B>(request: ureq::RequestBuilder<B>, server: &Server) -> ureq::RequestBuilder<B> {
    if server.api_key.trim().is_empty() {
        request
    } else {
        request.header("Authorization", &format!("Bearer {}", server.api_key.trim()))
    }
}

pub fn list_models(server: &Server) -> Result<Vec<String>, String> {
    let request = agent(Some(Duration::from_secs(5))).get(format!("{}/models", base(server)));
    let mut response = with_key(request, server).call().map_err(|e| e.to_string())?;
    let status = response.status();
    let body: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(body["error"]["message"].as_str().map(String::from).unwrap_or_else(|| format!("HTTP {status}")));
    }
    let mut ids: Vec<String> = body["data"]
        .as_array()
        .map(|list| list.iter().filter_map(|item| item["id"].as_str().map(String::from)).collect())
        .unwrap_or_default();
    ids.sort();
    Ok(ids)
}

pub fn to_openai(messages: &[Message]) -> Vec<Value> {
    let mut counter = 0;
    let mut open_ids: VecDeque<String> = VecDeque::new();
    messages
        .iter()
        .map(|message| match message.role.as_str() {
            "tool" => {
                let id = open_ids.pop_front().unwrap_or_else(|| "call_0".to_string());
                json!({ "role": "tool", "tool_call_id": id, "content": message.content })
            }
            "assistant" if !message.tool_calls.is_empty() => {
                let calls: Vec<Value> = message
                    .tool_calls
                    .iter()
                    .map(|call| {
                        counter += 1;
                        let id = format!("call_{counter}");
                        open_ids.push_back(id.clone());
                        json!({
                            "id": id,
                            "type": "function",
                            "function": { "name": call.function.name, "arguments": call.function.arguments.to_string() }
                        })
                    })
                    .collect();
                json!({ "role": "assistant", "content": message.content, "tool_calls": calls })
            }
            role if !message.images.is_empty() => {
                let mut parts = vec![json!({ "type": "text", "text": message.content })];
                for image in &message.images {
                    parts.push(json!({ "type": "image_url", "image_url": { "url": format!("data:image/png;base64,{image}") } }));
                }
                json!({ "role": role, "content": parts })
            }
            role => json!({ "role": role, "content": message.content }),
        })
        .collect()
}

#[derive(Default)]
struct PartialCall {
    name: String,
    arguments: String,
}

pub fn chat_stream(
    server: &Server,
    model: &str,
    messages: &[Message],
    tools: Option<&Value>,
    cancel: &AtomicBool,
    mut on_chunk: impl FnMut(&str, bool),
) -> Result<ChatOutcome, String> {
    let mut body = json!({
        "model": model,
        "messages": to_openai(messages),
        "stream": true,
        "stream_options": { "include_usage": true },
    });
    if let Some(tools) = tools.filter(|_| server.tools) {
        body["tools"] = tools.clone();
    }
    let request = agent(None).post(format!("{}/chat/completions", base(server)));
    let response = with_key(request, server).send_json(&body).map_err(|e| e.to_string())?;
    let status = response.status();
    let mut reader = BufReader::new(response.into_body().into_reader());
    if !status.is_success() {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut reader, &mut text);
        let message = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|value| value["error"]["message"].as_str().map(String::from))
            .unwrap_or(text);
        return Err(format!("{}: {}", server.name, message.trim()));
    }
    let mut calls: Vec<PartialCall> = Vec::new();
    let mut stats = None;
    let mut line = String::new();
    loop {
        if cancel.load(Ordering::SeqCst) {
            break;
        }
        line.clear();
        if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let Some(data) = line.trim().strip_prefix("data:").map(str::trim) else { continue };
        if data == "[DONE]" {
            break;
        }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else { continue };
        if let Some(usage) = chunk.get("usage").filter(|usage| usage.is_object()) {
            stats = Some(ChatStats {
                prompt_tokens: usage["prompt_tokens"].as_u64().unwrap_or(0),
                output_tokens: usage["completion_tokens"].as_u64().unwrap_or(0),
            });
        }
        let delta = &chunk["choices"][0]["delta"];
        for key in ["reasoning_content", "reasoning"] {
            if let Some(text) = delta[key].as_str().filter(|text| !text.is_empty()) {
                on_chunk(text, true);
            }
        }
        if let Some(text) = delta["content"].as_str().filter(|text| !text.is_empty()) {
            on_chunk(text, false);
        }
        for call in delta["tool_calls"].as_array().into_iter().flatten() {
            let index = call["index"].as_u64().unwrap_or(calls.len() as u64) as usize;
            while calls.len() <= index {
                calls.push(PartialCall::default());
            }
            if let Some(name) = call["function"]["name"].as_str() {
                calls[index].name.push_str(name);
            }
            if let Some(arguments) = call["function"]["arguments"].as_str() {
                calls[index].arguments.push_str(arguments);
            }
        }
    }
    let tool_calls = calls
        .into_iter()
        .filter(|call| !call.name.is_empty())
        .map(|call| ToolCall {
            function: ToolFunction {
                name: call.name,
                arguments: serde_json::from_str(&call.arguments).unwrap_or_else(|_| json!({})),
            },
        })
        .collect();
    Ok(ChatOutcome { stats, tool_calls })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_messages() {
        let server = Server { name: "LM Studio".into(), url: "http://localhost:1234/v1/".into(), enabled: true, tools: true, ..Default::default() };
        let name = model_name(&server, "qwen3-8b");
        assert_eq!(name, "⇄ LM Studio: qwen3-8b");
        assert_eq!(from_model(&name, std::slice::from_ref(&server)).unwrap().1, "qwen3-8b");
        assert!(from_model("qwen3:8b", std::slice::from_ref(&server)).is_none());
        assert_eq!(base(&server), "http://localhost:1234/v1");
        let call = ToolCall { function: ToolFunction { name: "calculate".into(), arguments: json!({"expression": "1+1"}) } };
        let messages = vec![
            Message { role: "user".into(), content: "hi".into(), ..Default::default() },
            Message { role: "assistant".into(), content: "".into(), tool_calls: vec![call], ..Default::default() },
            Message { role: "tool".into(), content: "2".into(), tool_name: Some("calculate".into()), ..Default::default() },
        ];
        let out = to_openai(&messages);
        assert_eq!(out[1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(out[1]["tool_calls"][0]["function"]["arguments"], "{\"expression\":\"1+1\"}");
        assert_eq!(out[2]["tool_call_id"], "call_1");
    }
}
