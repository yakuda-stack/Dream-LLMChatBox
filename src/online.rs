use regex::Regex;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

const OLLAMA_URL: &str = "https://ollama.com";
const HF_URL: &str = "https://huggingface.co";
const USER_AGENT: &str = "Dream-LLMChatBox";

pub struct OnlineModel {
    pub id: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub param_sizes: Vec<String>,
    pub downloads: String,
}

pub struct OnlineVariant {
    pub tag: String,
    pub label: String,
    pub size: u64,
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(USER_AGENT)
        .build()
        .into()
}

fn get_text(url: &str, query: &[(&str, &str)]) -> Result<String, String> {
    let mut request = agent().get(url);
    for (key, value) in query {
        request = request.query(*key, *value);
    }
    let mut response = request.call().map_err(|e| e.to_string())?;
    response
        .body_mut()
        .with_config()
        .limit(20 * 1024 * 1024)
        .read_to_string()
        .map_err(|e| e.to_string())
}

fn regex(pattern: &'static str, cell: &'static OnceLock<Regex>) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid regex"))
}

fn strip_tags(html: &str) -> String {
    static TAGS: OnceLock<Regex> = OnceLock::new();
    let text = regex(r"<[^>]*>", &TAGS).replace_all(html, " ");
    let text = text
        .replace("&amp;", "&")
        .replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn search_ollama(query: &str) -> Result<Vec<OnlineModel>, String> {
    let html = get_text(&format!("{OLLAMA_URL}/search"), &[("q", query)])?;
    Ok(parse_ollama_search(&html))
}

fn parse_ollama_search(html: &str) -> Vec<OnlineModel> {
    static NAME: OnceLock<Regex> = OnceLock::new();
    static DESCRIPTION: OnceLock<Regex> = OnceLock::new();
    static CAPABILITY: OnceLock<Regex> = OnceLock::new();
    static SIZE: OnceLock<Regex> = OnceLock::new();
    static DOWNLOADS: OnceLock<Regex> = OnceLock::new();

    let name_re = regex(r#"<a\s+href="/library/([^"/:]+)""#, &NAME);
    let description_re = regex(r"(?s)<p[^>]*>(.*?)</p>", &DESCRIPTION);
    let capability_re = regex(
        r#"<span\s+class="[^"]*bg-indigo-50[^"]*"[^>]*>([^<]+)</span>"#,
        &CAPABILITY,
    );
    let size_re = regex(r#"<span\s+class="[^"]*bg-\[#ddf4ff\][^"]*"[^>]*>([^<]+)</span>"#, &SIZE);
    let downloads_re = regex(r#"title="([\d,\.]+) downloads""#, &DOWNLOADS);

    html.split("<li")
        .filter_map(|block| {
            let id = name_re.captures(block)?.get(1)?.as_str().to_string();
            let description = description_re
                .captures(block)
                .map(|c| strip_tags(&c[1]))
                .unwrap_or_default();
            let capabilities = capability_re
                .captures_iter(block)
                .map(|c| c[1].trim().to_string())
                .collect();
            let param_sizes = size_re
                .captures_iter(block)
                .map(|c| c[1].trim().to_string())
                .collect();
            let downloads = downloads_re
                .captures(block)
                .map(|c| c[1].to_string())
                .unwrap_or_default();
            Some(OnlineModel {
                id,
                description,
                capabilities,
                param_sizes,
                downloads,
            })
        })
        .collect()
}

pub fn ollama_variants(name: &str) -> Result<Vec<OnlineVariant>, String> {
    let html = get_text(&format!("{OLLAMA_URL}/library/{name}/tags"), &[])?;
    Ok(parse_ollama_tags(&html))
}

fn parse_ollama_tags(html: &str) -> Vec<OnlineVariant> {
    static TAG: OnceLock<Regex> = OnceLock::new();
    static PLAIN: OnceLock<Regex> = OnceLock::new();
    static GB: OnceLock<Regex> = OnceLock::new();

    let tag_re = regex(r#"<a\s+href="/library/([^"]+:[^"]+)"\s+class="md:hidden"#, &TAG);
    let plain_re = regex(r"^(?:e?\d+(?:\.\d+)?[bm](?:-a\d+(?:\.\d+)?b)?|latest)$", &PLAIN);
    let size_re = regex(r"(\d+(?:\.\d+)?)\s*(GB|MB)", &GB);

    let mut variants: Vec<OnlineVariant> = Vec::new();
    let starts: Vec<(usize, String)> = tag_re
        .captures_iter(html)
        .filter_map(|c| Some((c.get(0)?.start(), c.get(1)?.as_str().to_string())))
        .collect();

    for (index, (start, full_tag)) in starts.iter().enumerate() {
        let end = starts
            .get(index + 1)
            .map(|next| next.0)
            .unwrap_or(html.len());
        let block = &html[*start..end];
        let block = block.split("</a>").next().unwrap_or(block);
        let label = full_tag.split_once(':').map(|(_, t)| t).unwrap_or("");
        if !plain_re.is_match(label) || variants.iter().any(|v| v.tag == *full_tag) {
            continue;
        }
        let text = strip_tags(block);
        let text = text.split("context").next().unwrap_or(&text);
        let size = size_re
            .captures_iter(text)
            .filter_map(|c| {
                let value: f64 = c[1].parse().ok()?;
                let factor = if &c[2] == "GB" { 1e9 } else { 1e6 };
                Some((value * factor) as u64)
            })
            .max();
        if let Some(size) = size {
            variants.push(OnlineVariant {
                tag: full_tag.clone(),
                label: label.to_string(),
                size,
            });
        }
    }

    let latest_size = variants.iter().find(|v| v.label == "latest").map(|v| v.size);
    if variants.len() > 1
        && let Some(size) = latest_size
        && variants.iter().any(|v| v.label != "latest" && v.size == size)
    {
        variants.retain(|v| v.label != "latest");
    }
    variants
}

#[derive(Deserialize)]
struct HfModel {
    id: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    pipeline_tag: Option<String>,
}

#[derive(Deserialize)]
struct HfFile {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    size: u64,
}

pub fn search_hf(query: &str) -> Result<Vec<OnlineModel>, String> {
    let json = get_text(
        &format!("{HF_URL}/api/models"),
        &[
            ("search", query),
            ("filter", "gguf"),
            ("sort", "downloads"),
            ("limit", "50"),
        ],
    )?;
    let models: Vec<HfModel> = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    Ok(models
        .into_iter()
        .map(|model| {
            let mut capabilities = Vec::new();
            let is_vision = model.pipeline_tag.as_deref() == Some("image-text-to-text")
                || model.tags.iter().any(|t| t == "vision" || t == "multimodal");
            if is_vision {
                capabilities.push("vision".to_string());
            }
            if model.tags.iter().any(|t| t == "reasoning" || t == "thinking") {
                capabilities.push("thinking".to_string());
            }
            let repo_name = model.id.rsplit('/').next().unwrap_or(&model.id);
            let param_sizes = crate::hardware::parse_params(repo_name)
                .map(|b| vec![format!("{b}b")])
                .unwrap_or_default();
            OnlineModel {
                description: crate::i18n::trf("Hugging Face GGUF repository by {}", &[&owner(&model.id)]),
                id: model.id,
                capabilities,
                param_sizes,
                downloads: format_count(model.downloads),
            }
        })
        .collect())
}

fn owner(repo: &str) -> &str {
    repo.split('/').next().unwrap_or(repo)
}

fn format_count(count: u64) -> String {
    match count {
        c if c >= 1_000_000 => format!("{:.1}M", c as f64 / 1e6),
        c if c >= 1_000 => format!("{:.1}K", c as f64 / 1e3),
        c => c.to_string(),
    }
}

pub fn hf_variants(repo: &str) -> Result<Vec<OnlineVariant>, String> {
    let json = get_text(
        &format!("{HF_URL}/api/models/{repo}/tree/main"),
        &[("recursive", "true")],
    )?;
    let files: Vec<HfFile> = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    Ok(group_gguf_files(repo, &files))
}

fn group_gguf_files(repo: &str, files: &[HfFile]) -> Vec<OnlineVariant> {
    static QUANT: OnceLock<Regex> = OnceLock::new();
    let quant_re = regex(
        r"(?i)(?:^|[-_./])((?:I?Q\d(?:_[A-Z0-9]+)*)|BF16|F16|F32|MXFP4|UD-[A-Z0-9_]+)(?:[-_.]|$)",
        &QUANT,
    );

    let mut sizes: BTreeMap<String, u64> = BTreeMap::new();
    for file in files {
        let lower = file.path.to_lowercase();
        if file.kind != "file" || !lower.ends_with(".gguf") || lower.contains("mmproj") {
            continue;
        }
        let quant = quant_re
            .captures_iter(&file.path)
            .last()
            .map(|c| c[1].to_uppercase());
        if let Some(quant) = quant {
            *sizes.entry(quant).or_insert(0) += file.size;
        }
    }

    let mut variants: Vec<OnlineVariant> = sizes
        .into_iter()
        .map(|(quant, size)| OnlineVariant {
            tag: format!("hf.co/{repo}:{quant}"),
            label: quant,
            size,
        })
        .collect();
    variants.sort_by_key(|v| v.size);
    variants
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_search() {
        let html = r#"<ul><li class="border-t">
  <a href="/library/qwen3.8" class="group grid">
    <h2 title="qwen3.8"><span>qwen3.8</span></h2>
    <div><span  class="inline-flex items-center rounded-md bg-indigo-50 px-2">vision</span><span class="inline-flex items-center rounded-md bg-indigo-50 px-2">tools</span></div>
    <p class="mt-1 line-clamp-2">Qwen3.8 delivers substantial gains &amp; more.</p>
    <div><span class="inline-flex" title="6,615,549 downloads"><span>6.6M</span></span>
      <span  class="inline-flex items-center rounded-md bg-[#ddf4ff] px-2">27b</span>
      <span class="inline-flex items-center rounded-md bg-[#ddf4ff] px-2">4b</span></div>
  </a>
</li></ul>"#;
        let models = parse_ollama_search(html);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].id, "qwen3.8");
        assert_eq!(models[0].capabilities, vec!["vision", "tools"]);
        assert_eq!(models[0].param_sizes, vec!["27b", "4b"]);
        assert_eq!(models[0].description, "Qwen3.8 delivers substantial gains & more.");
        assert_eq!(models[0].downloads, "6,615,549");
    }

    #[test]
    fn ollama_tags() {
        let html = r#"
<div class="group px-4 py-3">
  <a href="/library/qwen3.5:latest" class="md:hidden flex flex-col">
    <span class="group-hover:underline">qwen3.5:latest</span>
    <span><span class="font-mono">fdf5fd4cd409</span> • 6.6GB - 7.6GB • 256K context window •</span>
  </a>
</div>
<div class="group px-4 py-3">
  <a href="/library/qwen3.5:2b" class="md:hidden flex flex-col">
    <span><span class="font-mono">aaa</span> • 2.7GB • 256K context window •</span>
  </a>
</div>
<div class="group px-4 py-3">
  <a href="/library/qwen3.5:9b" class="md:hidden flex flex-col">
    <span><span class="font-mono">bbb</span> • 6.6GB - 7.6GB • 256K context window •</span>
  </a>
</div>
<div class="group px-4 py-3">
  <a href="/library/qwen3.5:9b-q8_0" class="md:hidden flex flex-col">
    <span><span class="font-mono">ccc</span> • 11GB • 256K context window •</span>
  </a>
</div>
<div class="group px-4 py-3">
  <a href="/library/qwen3.5:35b-a3b" class="md:hidden flex flex-col">
    <span><span class="font-mono">ddd</span> • 22GB • 256K context window •</span>
  </a>
</div>"#;
        let variants = parse_ollama_tags(html);
        let labels: Vec<&str> = variants.iter().map(|v| v.label.as_str()).collect();
        assert_eq!(labels, vec!["2b", "9b", "35b-a3b"]);
        assert_eq!(variants[1].size, 7_600_000_000);
    }

    #[test]
    fn hf_files() {
        let file = |path: &str, size: u64| HfFile {
            kind: "file".to_string(),
            path: path.to_string(),
            size,
        };
        let files = vec![
            file("Model-Q4_K_M.gguf", 100),
            file("Model-Q8_0.gguf", 200),
            file("mmproj-model-f16.gguf", 5),
            file("BF16/Model-BF16-00001-of-00002.gguf", 300),
            file("BF16/Model-BF16-00002-of-00002.gguf", 300),
            file("Model.IQ3_XXS.gguf", 50),
            file("README.md", 1),
        ];
        let variants = group_gguf_files("user/Model-GGUF", &files);
        let labels: Vec<(&str, u64)> = variants.iter().map(|v| (v.label.as_str(), v.size)).collect();
        assert_eq!(
            labels,
            vec![("IQ3_XXS", 50), ("Q4_K_M", 100), ("Q8_0", 200), ("BF16", 600)]
        );
        assert_eq!(variants[1].tag, "hf.co/user/Model-GGUF:Q4_K_M");
    }
}
