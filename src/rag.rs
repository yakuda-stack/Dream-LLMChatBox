use crate::ollama;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, UNIX_EPOCH};

pub const DEFAULT_MODEL: &str = "nomic-embed-text";
const CHUNK_CHARS: usize = 1200;
const BATCH: usize = 16;
const MAX_FILE_BYTES: u64 = 20_000_000;
const MAX_FILES: usize = 3000;
const MIN_SCORE: f32 = 0.2;
const SKIP: [&str; 6] = [".git", "target", "node_modules", ".venv", "__pycache__", "build"];
const TEXT: [&str; 22] = [
    "txt", "md", "markdown", "rst", "csv", "json", "toml", "yaml", "yml", "html", "htm", "xml", "rs", "py", "js",
    "ts", "c", "h", "cpp", "java", "go", "sh",
];

#[derive(Serialize, Deserialize, Clone)]
pub struct Chunk {
    pub path: String,
    pub line: usize,
    pub text: String,
    pub vector: Vec<f32>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Index {
    pub model: String,
    pub files: HashMap<String, u64>,
    pub chunks: Vec<Chunk>,
}

pub struct Stats {
    pub files: usize,
    pub chunks: usize,
    pub changed: usize,
}

fn index_path(folder: &Path) -> PathBuf {
    let mut hash: u64 = 1469598103934665603;
    for byte in folder.to_string_lossy().bytes() {
        hash = (hash ^ byte as u64).wrapping_mul(1099511628211);
    }
    crate::storage::data_dir().join("rag").join(format!("{hash:016x}.json"))
}

pub fn load(folder: &Path) -> Option<Index> {
    serde_json::from_str(&fs::read_to_string(index_path(folder)).ok()?).ok()
}

fn save(folder: &Path, index: &Index) -> Result<(), String> {
    let path = index_path(folder);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, serde_json::to_string(index).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn ext(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn supported(path: &Path) -> bool {
    let ext = ext(path);
    TEXT.contains(&ext.as_str()) || ["pdf", "docx", "odt"].contains(&ext.as_str())
}

pub fn strip_xml(xml: &str) -> String {
    let xml = xml.replace("</w:p>", "\n").replace("</text:p>", "\n").replace("<w:tab/>", "\t");
    let mut out = String::new();
    let mut inside = false;
    for c in xml.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            _ if !inside => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'")
}

fn unzip_entry(path: &Path, entry: &str) -> Option<String> {
    let output = Command::new("unzip").args(["-p"]).arg(path).arg(entry).output().ok()?;
    output.status.success().then(|| strip_xml(&String::from_utf8_lossy(&output.stdout)))
}

pub fn extract(path: &Path) -> Option<String> {
    match ext(path).as_str() {
        "pdf" => {
            let output = Command::new("pdftotext").arg("-layout").arg(path).arg("-").output().ok()?;
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).replace('\u{c}', "\n"))
        }
        "docx" => unzip_entry(path, "word/document.xml"),
        "odt" => unzip_entry(path, "content.xml"),
        _ => {
            let bytes = fs::read(path).ok()?;
            if bytes.iter().take(8000).any(|byte| *byte == 0) {
                return None;
            }
            Some(String::from_utf8_lossy(&bytes).to_string())
        }
    }
}

pub fn chunks_of(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut start = 1;
    for (number, line) in text.lines().enumerate() {
        if current.is_empty() {
            start = number + 1;
        }
        if current.len() + line.len() > CHUNK_CHARS && !current.trim().is_empty() {
            out.push((start, current.trim().to_string()));
            current.clear();
            start = number + 1;
        }
        let line: String = line.chars().take(CHUNK_CHARS).collect();
        current.push_str(&line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        out.push((start, current.trim().to_string()));
    }
    out
}

fn files_in(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = entry.file_name();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if !SKIP.iter().any(|skip| name == *skip) && !name.to_string_lossy().starts_with('.') {
                files_in(&entry.path(), out);
            }
        } else if kind.is_file() && supported(&entry.path()) {
            out.push(entry.path());
        }
    }
}

fn mtime(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |time| time.as_secs())
}

pub fn update(folder: &Path, model: &str, limit: Option<Duration>, progress: &dyn Fn(usize, usize)) -> Result<Stats, String> {
    let started = Instant::now();
    let mut index = load(folder).filter(|index| index.model == model).unwrap_or_else(|| Index {
        model: model.to_string(),
        ..Default::default()
    });
    let mut files = Vec::new();
    files_in(folder, &mut files);
    let present: Vec<String> = files.iter().map(|path| path.to_string_lossy().to_string()).collect();
    index.chunks.retain(|chunk| present.contains(&chunk.path));
    index.files.retain(|path, _| present.contains(path));
    let changed: Vec<&PathBuf> = files
        .iter()
        .filter(|path| {
            let key = path.to_string_lossy().to_string();
            index.files.get(&key) != Some(&mtime(path)) && fs::metadata(path).is_ok_and(|meta| meta.len() <= MAX_FILE_BYTES)
        })
        .collect();
    let total = changed.len();
    let mut done = 0;
    for path in changed {
        if limit.is_some_and(|limit| started.elapsed() > limit) {
            break;
        }
        let key = path.to_string_lossy().to_string();
        index.chunks.retain(|chunk| chunk.path != key);
        let pieces = extract(path).map(|text| chunks_of(&text)).unwrap_or_default();
        for batch in pieces.chunks(BATCH) {
            let inputs: Vec<String> = batch.iter().map(|(_, text)| text.clone()).collect();
            let vectors = ollama::embed(model, &inputs)?;
            for ((line, text), vector) in batch.iter().zip(vectors) {
                index.chunks.push(Chunk { path: key.clone(), line: *line, text: text.clone(), vector });
            }
        }
        index.files.insert(key, mtime(path));
        done += 1;
        progress(done, total);
    }
    save(folder, &index)?;
    Ok(Stats {
        files: index.files.len(),
        chunks: index.chunks.len(),
        changed: done,
    })
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm = |v: &[f32]| v.iter().map(|x| x * x).sum::<f32>().sqrt();
    let n = norm(a) * norm(b);
    if n == 0.0 { 0.0 } else { dot / n }
}

pub fn best<'a>(index: &'a Index, query: &[f32], count: usize) -> Vec<(f32, &'a Chunk)> {
    let mut scored: Vec<(f32, &Chunk)> = index.chunks.iter().map(|chunk| (cosine(query, &chunk.vector), chunk)).collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    scored.truncate(count);
    scored
}

pub fn search(folder: &Path, model: &str, query: &str, count: usize) -> Result<String, String> {
    let note = match update(folder, model, Some(Duration::from_secs(60)), &|_, _| {}) {
        Ok(stats) if stats.changed > 0 => format!(" ({} changed files indexed just now)", stats.changed),
        Ok(_) => String::new(),
        Err(e) => return Err(explain(&e, model)),
    };
    let index = load(folder).ok_or("No index yet – press “Index now” in the group settings")?;
    if index.chunks.is_empty() {
        return Ok(format!("The document folder {} has no readable documents.", folder.display()));
    }
    let query_vector = ollama::embed(model, &[query.to_string()]).map_err(|e| explain(&e, model))?;
    let mut out = format!("Best matches in {}{note}:\n", folder.display());
    let found = best(&index, &query_vector[0], count);
    let top = found.first().map_or(0.0, |(score, _)| *score);
    for (score, chunk) in found.into_iter().filter(|(score, _)| *score >= MIN_SCORE || *score >= top) {
        let shown = Path::new(&chunk.path)
            .strip_prefix(folder)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| chunk.path.clone());
        let text: String = chunk.text.chars().take(900).collect();
        out.push_str(&format!("\n[{shown}, line {}, score {:.2}]\n{text}\n", chunk.line, score));
    }
    Ok(out)
}

pub fn explain(error: &str, model: &str) -> String {
    if error.contains("not found") || error.contains("pull") {
        format!("The embedding model {model} is not installed – run: ollama pull {model}")
    } else {
        format!("Document search failed: {error}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_xml_and_scores() {
        let text = format!("{}\n{}\nend", "a".repeat(800), "b".repeat(800));
        let pieces = chunks_of(&text);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[1].0, 2);
        assert_eq!(strip_xml("<w:p><w:t>Hallo &amp; Tschüss</w:t></w:p>"), "Hallo & Tschüss\n");
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        let index = Index {
            model: "m".into(),
            files: HashMap::new(),
            chunks: vec![
                Chunk { path: "a".into(), line: 1, text: "cats".into(), vector: vec![1.0, 0.0] },
                Chunk { path: "b".into(), line: 1, text: "cars".into(), vector: vec![0.0, 1.0] },
            ],
        };
        assert_eq!(best(&index, &[0.1, 0.9], 1)[0].1.text, "cars");
        assert!(supported(Path::new("x.PDF")) && supported(Path::new("a.docx")) && !supported(Path::new("a.png")));
    }
}
