use crate::sandbox::{self, Folder};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub const NAME: &str = "create_document";
const FORMATS: [&str; 8] = ["pdf", "docx", "odt", "pptx", "html", "epub", "md", "txt"];
const PDF_ENGINES: [&str; 5] = ["typst", "weasyprint", "wkhtmltopdf", "xelatex", "pdflatex"];
const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_BYTES: usize = 1_000_000;

pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": "Create a finished document from Markdown inside a shared folder that allows writing. The file ending of path picks the format: .pdf, .docx (Word), .odt, .pptx (slides: every '# Heading' or '---' starts a new slide), .html, .epub or .md. Use Markdown headings, lists, tables and **bold**.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path with ending, relative to the shared folder, e.g. docs/report.pdf" },
                    "content": { "type": "string", "description": "The complete document as Markdown" },
                    "title": { "type": "string", "description": "Optional document title" }
                },
                "required": ["path", "content"]
            }
        }
    })
}

pub fn format_of(path: &str) -> Option<&'static str> {
    let ending = Path::new(path).extension()?.to_str()?.to_lowercase();
    FORMATS.iter().copied().find(|format| *format == ending)
}

pub fn which(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
    })
}

pub fn status() -> String {
    let mut parts = vec![if which("pandoc") { "pandoc ✓" } else { "pandoc ✗" }.to_string()];
    match PDF_ENGINES.iter().find(|engine| which(engine)) {
        Some(engine) => parts.push(crate::i18n::trf("PDF via {}", &[engine])),
        None if which("soffice") || which("libreoffice") => parts.push(crate::i18n::tr("PDF via LibreOffice")),
        None => parts.push(crate::i18n::tr("no PDF engine")),
    }
    parts.join("  ·  ")
}

pub fn run(arguments: &Value, folders: &[Folder]) -> String {
    match create(arguments, folders) {
        Ok(text) => text,
        Err(e) => format!("Error: {e}"),
    }
}

fn create(arguments: &Value, folders: &[Folder]) -> Result<String, String> {
    let path = arguments["path"].as_str().unwrap_or_default();
    let content = arguments["content"].as_str().unwrap_or_default();
    let title = arguments["title"].as_str().unwrap_or_default().trim();
    let format = format_of(path).ok_or_else(|| {
        format!("Unknown file ending in '{path}'. Use one of: {}", FORMATS.join(", "))
    })?;
    if content.trim().is_empty() {
        return Err("content is empty".to_string());
    }
    if content.len() > MAX_BYTES {
        return Err("Content is too big (max. 1 MB)".to_string());
    }
    let target = sandbox::resolve(folders, path)?;
    if !target.writable {
        return Err(format!("{} is read only, the user did not allow writing", target.shown()));
    }
    if target.path.is_dir() {
        return Err(format!("{} is a folder", target.shown()));
    }
    if let Some(parent) = target.path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", target.shown()))?;
    }
    let existed = target.path.exists();
    let text = with_title(content, title);
    if format == "md" || format == "txt" {
        fs::write(&target.path, &text).map_err(|e| format!("{}: {e}", target.shown()))?;
    } else {
        convert(&text, format, &target.path)?;
    }
    let size = fs::metadata(&target.path).map(|meta| meta.len()).unwrap_or(0);
    Ok(format!(
        "{} {} ({}, {})",
        if existed { "Overwrote" } else { "Created" },
        target.shown(),
        format.to_uppercase(),
        human_size(size)
    ))
}

fn with_title(content: &str, title: &str) -> String {
    if title.is_empty() || content.trim_start().starts_with("---") {
        return content.to_string();
    }
    let escaped = title.replace('\\', "\\\\").replace('"', "\\\"");
    format!("---\ntitle: \"{escaped}\"\n---\n\n{content}")
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else {
        format!("{} KB", bytes.div_ceil(1024).max(1))
    }
}

fn convert(text: &str, format: &str, target: &Path) -> Result<(), String> {
    if !which("pandoc") {
        return Err("pandoc is not installed. Install it with your package manager (package 'pandoc'), then try again.".to_string());
    }
    let work = std::env::temp_dir().join(format!("dream-doc-{}", std::process::id()));
    fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let source = work.join("document.md");
    fs::write(&source, text).map_err(|e| e.to_string())?;
    let result = if format == "pdf" {
        pdf(&source, &work, target)
    } else {
        pandoc(&source, target, &[])
    };
    let _ = fs::remove_dir_all(&work);
    result
}

fn pdf(source: &Path, work: &Path, target: &Path) -> Result<(), String> {
    let mut errors = Vec::new();
    for engine in PDF_ENGINES.iter().filter(|engine| which(engine)) {
        let mut extra = vec![format!("--pdf-engine={engine}")];
        if engine.ends_with("latex") {
            extra.push("-V".to_string());
            extra.push("geometry:margin=2.5cm".to_string());
        }
        match pandoc(source, target, &extra) {
            Ok(()) => return Ok(()),
            Err(e) => errors.push(format!("{engine}: {e}")),
        }
    }
    let office = ["soffice", "libreoffice"].into_iter().find(|program| which(program));
    if let Some(office) = office {
        let docx = work.join("document.docx");
        pandoc(source, &docx, &[])?;
        let mut command = Command::new(office);
        command
            .args(["--headless", "--convert-to", "pdf", "--outdir"])
            .arg(work)
            .arg(&docx);
        wait(command)?;
        let made = work.join("document.pdf");
        if made.exists() {
            fs::copy(&made, target).map_err(|e| e.to_string())?;
            return Ok(());
        }
        errors.push("LibreOffice made no PDF".to_string());
    }
    if errors.is_empty() {
        return Err("No PDF engine found. Install one of: typst, weasyprint, wkhtmltopdf, texlive (xelatex) or LibreOffice.".to_string());
    }
    Err(errors.join("; "))
}

fn pandoc(source: &Path, target: &Path, extra: &[String]) -> Result<(), String> {
    let mut command = Command::new("pandoc");
    command
        .arg(source)
        .args(["-f", "markdown", "-s", "-o"])
        .arg(target)
        .args(extra);
    wait(command)
}

fn wait(mut command: Command) -> Result<(), String> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if status.success() {
                return Ok(());
            }
            let mut message = String::new();
            if let Some(mut stderr) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut stderr, &mut message);
            }
            let short: String = message.trim().chars().take(400).collect();
            return Err(if short.is_empty() { format!("failed ({status})") } else { short });
        }
        if start.elapsed() > TIMEOUT {
            let _ = child.kill();
            return Err("took longer than 2 minutes".to_string());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_from_ending() {
        assert_eq!(format_of("a/report.PDF"), Some("pdf"));
        assert_eq!(format_of("slides.pptx"), Some("pptx"));
        assert_eq!(format_of("notes"), None);
        assert_eq!(format_of("x.exe"), None);
    }

    #[test]
    fn title_goes_into_front_matter() {
        assert_eq!(with_title("Hi", ""), "Hi");
        assert_eq!(with_title("Hi", "A \"B\""), "---\ntitle: \"A \\\"B\\\"\"\n---\n\nHi");
        assert_eq!(with_title("---\ntitle: x\n---\nHi", "Other"), "---\ntitle: x\n---\nHi");
    }

    #[test]
    fn creates_documents() {
        let dir = std::env::temp_dir().join(format!("dream-doc-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let folders = [Folder { path: dir.display().to_string(), writable: true }];
        let made = run(&json!({"path": "out/a.md", "content": "# Hi"}), &folders);
        assert!(made.starts_with("Created"), "{made}");
        assert!(run(&json!({"path": "a.xyz", "content": "x"}), &folders).starts_with("Error"));
        if which("pandoc") {
            let made = run(&json!({"path": "b.docx", "content": "# Hi\n\n- a\n- b", "title": "T"}), &folders);
            assert!(made.contains("DOCX"), "{made}");
            assert!(dir.join("b.docx").exists());
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
