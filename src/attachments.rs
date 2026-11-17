use crate::i18n::{tr, trf};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::process::Command;

const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_FOLDER_FILES: usize = 300;
const MAX_FOLDER_BYTES: u64 = 4 * 1024 * 1024;

const IMAGE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "gif", "bmp"];
const AUDIO_EXTENSIONS: [&str; 8] = ["mp3", "wav", "ogg", "opus", "flac", "m4a", "aac", "wma"];
const VIDEO_EXTENSIONS: [&str; 7] = ["mp4", "mkv", "webm", "mov", "avi", "m4v", "flv"];
const SKIP_DIRS: [&str; 8] = [
    ".git",
    "target",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "dist",
    "build",
];

#[derive(Serialize, Deserialize, Clone)]
pub struct Attachment {
    pub name: String,
    pub kind: String,
    pub data: String,
}

pub enum Kind {
    Image,
    Media,
    Text,
}

fn extension(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

pub fn kind_of(path: &Path) -> Kind {
    let ext = extension(path);
    if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        Kind::Image
    } else if AUDIO_EXTENSIONS.contains(&ext.as_str()) || VIDEO_EXTENSIONS.contains(&ext.as_str())
    {
        Kind::Media
    } else {
        Kind::Text
    }
}

pub fn media_extensions() -> Vec<&'static str> {
    IMAGE_EXTENSIONS
        .iter()
        .chain(AUDIO_EXTENSIONS.iter())
        .chain(VIDEO_EXTENSIONS.iter())
        .copied()
        .collect()
}

const MAX_IMAGE_SIDE: u32 = 1536;

pub fn image(path: &Path) -> Result<Attachment, String> {
    let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_IMAGE_BYTES {
        return Err(trf("{} is larger than 20 MB", &[&file_name(path)]));
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    Ok(image_from_bytes(file_name(path), bytes))
}

pub fn image_from_bytes(name: String, bytes: Vec<u8>) -> Attachment {
    let bytes = shrink_image(&bytes).unwrap_or(bytes);
    Attachment {
        name,
        kind: "image".to_string(),
        data: base64::engine::general_purpose::STANDARD.encode(bytes),
    }
}

pub fn image_from_rgba(name: String, width: u32, height: u32, rgba: Vec<u8>) -> Option<Attachment> {
    let picture = image::RgbaImage::from_raw(width, height, rgba)?;
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(picture)
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
    Some(image_from_bytes(name, png.into_inner()))
}

fn shrink_image(bytes: &[u8]) -> Option<Vec<u8>> {
    let picture = image::load_from_memory(bytes).ok()?;
    let picture = if picture.width().max(picture.height()) > MAX_IMAGE_SIDE {
        picture.resize(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE, image::imageops::FilterType::Triangle)
    } else {
        picture
    };
    let rgb = picture.to_rgb8();
    let mut output = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 88)
        .encode_image(&rgb)
        .ok()?;
    Some(output.into_inner())
}

pub fn describe_image(name: &str, base64_data: &str) -> String {
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(base64_data) else {
        return format!("[Image {name}: could not be read]");
    };
    let Ok(picture) = image::load_from_memory(&bytes) else {
        return format!("[Image {name}: could not be decoded]");
    };
    let rgb = picture.to_rgb8();
    let (width, height) = rgb.dimensions();

    let columns: u32 = 24;
    let rows: u32 = ((columns as f64 * height as f64 / width.max(1) as f64).round() as u32).clamp(4, 24);
    let cell_w = (width as f64 / columns as f64).max(1.0);
    let cell_h = (height as f64 / rows as f64).max(1.0);

    let mut text = format!(
        "[Image {name}: {width}x{height} px. The model cannot see images, so this is a text description.]\n\
         Average color grid ({columns} columns x {rows} rows, each cell about {}x{} px, row y0 = top):\n",
        cell_w.round(),
        cell_h.round()
    );
    for row in 0..rows {
        let mut line = format!("y{row:02}:");
        for column in 0..columns {
            let x0 = (column as f64 * cell_w) as u32;
            let y0 = (row as f64 * cell_h) as u32;
            let x1 = (((column + 1) as f64 * cell_w) as u32).min(width);
            let y1 = (((row + 1) as f64 * cell_h) as u32).min(height);
            let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
            for y in (y0..y1.max(y0 + 1)).step_by(2) {
                for x in (x0..x1.max(x0 + 1)).step_by(2) {
                    if x < width && y < height {
                        let p = rgb.get_pixel(x, y);
                        r += p[0] as u64;
                        g += p[1] as u64;
                        b += p[2] as u64;
                        n += 1;
                    }
                }
            }
            let n = n.max(1);
            line.push_str(&format!(" #{:02x}{:02x}{:02x}", r / n, g / n, b / n));
        }
        text.push_str(&line);
        text.push('\n');
    }

    let mut buckets: std::collections::HashMap<(u8, u8, u8), u64> = std::collections::HashMap::new();
    for pixel in rgb.pixels().step_by(7) {
        *buckets
            .entry((pixel[0] & 0xe0, pixel[1] & 0xe0, pixel[2] & 0xe0))
            .or_insert(0) += 1;
    }
    let total: u64 = buckets.values().sum::<u64>().max(1);
    let mut dominant: Vec<_> = buckets.into_iter().collect();
    dominant.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    let colors: Vec<String> = dominant
        .iter()
        .take(6)
        .map(|((r, g, b), count)| {
            format!("#{:02x}{:02x}{:02x} {}%", r + 16, g + 16, b + 16, count * 100 / total)
        })
        .collect();
    text.push_str(&format!("Dominant colors: {}\n", colors.join(", ")));

    match ocr(&bytes) {
        Some(found) if !found.trim().is_empty() => {
            text.push_str(&format!("Text found in the image (OCR):\n{}\n", found.trim()));
        }
        Some(_) => text.push_str("No text found in the image (OCR).\n"),
        None => text.push_str("(No OCR: install 'tesseract' to also read text in images.)\n"),
    }
    text
}

fn ocr(bytes: &[u8]) -> Option<String> {
    let file = std::env::temp_dir().join(format!("dream-llmchatbox-ocr-{}.img", std::process::id()));
    fs::write(&file, bytes).ok()?;
    let run = |languages: Option<&str>| {
        let mut command = Command::new("tesseract");
        command.arg(&file).arg("stdout");
        if let Some(languages) = languages {
            command.args(["-l", languages]);
        }
        command
            .stderr(std::process::Stdio::null())
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).to_string())
    };
    let result = run(Some("deu+eng")).or_else(|| run(None));
    let _ = fs::remove_file(&file);
    result
}

pub fn text_file(path: &Path) -> Result<Attachment, String> {
    let name = file_name(path);
    if extension(path) == "pdf" {
        let output = Command::new("pdftotext")
            .arg(path)
            .arg("-")
            .output()
            .map_err(|_| trf("{}: PDF needs 'pdftotext' (poppler) installed", &[&name]))?;
        if !output.status.success() {
            return Err(trf("{}: could not read PDF", &[&name]));
        }
        return Ok(Attachment {
            name,
            kind: "text".to_string(),
            data: String::from_utf8_lossy(&output.stdout).to_string(),
        });
    }

    let size = fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > MAX_TEXT_BYTES {
        return Err(trf("{} is larger than 2 MB", &[&name]));
    }
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|_| trf("{} is not a text file", &[&name]))?;
    Ok(Attachment {
        name,
        kind: "text".to_string(),
        data: text,
    })
}

pub fn folder(path: &Path) -> Result<Attachment, String> {
    let mut files: Vec<(String, String)> = Vec::new();
    let mut total: u64 = 0;
    let mut skipped = 0usize;
    collect(path, path, &mut files, &mut total, &mut skipped)?;
    if files.is_empty() {
        return Err(trf("{}: no text files found", &[&file_name(path)]));
    }

    let mut data = String::new();
    for (relative, content) in &files {
        data.push_str(&format!("--- {relative} ---\n{content}\n\n"));
    }
    if skipped > 0 {
        data.push_str(&format!("({skipped} files skipped: binary, too large or limit reached)\n"));
    }
    Ok(Attachment {
        name: format!("{}/ ({} files)", file_name(path), files.len()),
        kind: "text".to_string(),
        data,
    })
}

fn collect(
    root: &Path,
    dir: &Path,
    files: &mut Vec<(String, String)>,
    total: &mut u64,
    skipped: &mut usize,
) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && name != ".gitignore" {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            if !SKIP_DIRS.contains(&name.as_str()) {
                collect(root, &path, files, total, skipped)?;
            }
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if files.len() >= MAX_FOLDER_FILES || *total + size > MAX_FOLDER_BYTES {
            *skipped += 1;
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            *skipped += 1;
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            *skipped += 1;
            continue;
        };
        *total += size;
        let relative = path
            .strip_prefix(root)
            .map(|p| p.display().to_string())
            .unwrap_or(name);
        files.push((relative, text));
    }
    Ok(())
}

pub fn decode_audio(path: &Path) -> Result<Vec<f32>, String> {
    let output = Command::new("ffmpeg")
        .args(["-nostdin", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-f", "f32le", "-"])
        .output()
        .map_err(|_| tr("Audio and video need 'ffmpeg' installed"))?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg: {}", error.trim()));
    }
    Ok(output
        .stdout
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect())
}

pub fn transcript(path: &Path, text: String) -> Attachment {
    Attachment {
        name: format!("{} (transcript)", file_name(path)),
        kind: "text".to_string(),
        data: text,
    }
}

pub fn to_json(attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        String::new()
    } else {
        serde_json::to_string(attachments).unwrap_or_default()
    }
}

pub fn from_json(json: &str) -> Vec<Attachment> {
    if json.is_empty() {
        Vec::new()
    } else {
        serde_json::from_str(json).unwrap_or_default()
    }
}

pub fn names(attachments: &[Attachment]) -> String {
    attachments
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join("  ·  ")
}

pub fn prompt_text(text: &str, attachments: &[Attachment]) -> String {
    let mut prompt = text.to_string();
    for attachment in attachments.iter().filter(|a| a.kind == "text") {
        prompt.push_str(&format!(
            "\n\n[Attached: {}]\n```\n{}\n```",
            attachment.name,
            attachment.data.trim_end()
        ));
    }
    prompt
}

pub fn image_names(attachments: &[Attachment]) -> Vec<String> {
    attachments
        .iter()
        .filter(|a| a.kind == "image")
        .map(|a| a.name.clone())
        .collect()
}

pub fn images(attachments: &[Attachment]) -> Vec<String> {
    attachments
        .iter()
        .filter(|a| a.kind == "image")
        .map(|a| a.data.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_reads_text_and_skips() {
        let dir = std::env::temp_dir().join(format!("dlc-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::create_dir_all(dir.join("target")).unwrap();
        fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        fs::write(dir.join("README.md"), "# Hi").unwrap();
        fs::write(dir.join("target/big.bin"), [0u8, 159, 146, 150]).unwrap();
        fs::write(dir.join("logo.bin"), [0u8, 159, 146, 150]).unwrap();

        let attachment = folder(&dir).unwrap();
        assert!(attachment.name.contains("(2 files)"));
        assert!(attachment.data.contains("--- src/main.rs ---"));
        assert!(attachment.data.contains("1 files skipped"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prompt_includes_text_only() {
        let list = vec![
            Attachment { name: "a.txt".into(), kind: "text".into(), data: "hello".into() },
            Attachment { name: "b.png".into(), kind: "image".into(), data: "AAAA".into() },
        ];
        let prompt = prompt_text("Question", &list);
        assert!(prompt.starts_with("Question"));
        assert!(prompt.contains("[Attached: a.txt]"));
        assert!(!prompt.contains("AAAA"));
        assert_eq!(images(&list), vec!["AAAA".to_string()]);
    }

    #[test]
    fn describes_image() {
        let mut picture = image::RgbImage::new(64, 32);
        for (x, _, pixel) in picture.enumerate_pixels_mut() {
            *pixel = if x < 32 { image::Rgb([255, 0, 0]) } else { image::Rgb([0, 0, 255]) };
        }
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(picture)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let data = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
        let text = describe_image("test.png", &data);
        assert!(text.contains("64x32 px"));
        assert!(text.contains("y00: #ff0000"));
        assert!(text.contains("#0000ff"));
        assert!(text.contains("Dominant colors"));
    }
}
