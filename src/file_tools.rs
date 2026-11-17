use crate::sandbox::{self, Folder};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

const SKIP_DIRS: [&str; 12] = [
    ".git",
    "target",
    "node_modules",
    "venv",
    ".venv",
    "__pycache__",
    "dist",
    "build",
    ".mypy_cache",
    ".pytest_cache",
    ".idea",
    ".cache",
];
const MAX_ENTRIES: usize = 300;
const MAX_LINES: usize = 300;
const MAX_LINE_CHARS: usize = 500;
const MAX_READ_BYTES: u64 = 2_000_000;
const MAX_WRITE_BYTES: usize = 1_000_000;
const MAX_MATCHES: usize = 100;
const MAX_SEARCH_FILES: usize = 5_000;

pub fn definitions(folders: &[Folder]) -> Vec<Value> {
    if folders.is_empty() {
        return Vec::new();
    }
    let mut list = vec![
        json!({
            "type": "function",
            "function": {
                "name": "list_folder",
                "description": "List files and subfolders inside a shared folder as a tree. Build folders like .git, target and node_modules are skipped.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Folder path, relative to the shared folder. Use . for the shared folder itself." },
                        "depth": { "type": "integer", "description": "How many levels deep, 1 to 4. Default 2." }
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a text file inside a shared folder. Returns numbered lines, at most 300 per call.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path, relative to the shared folder" },
                        "start_line": { "type": "integer", "description": "First line to read, default 1" }
                    },
                    "required": ["path"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "search_files",
                "description": "Search all text files in a shared folder for a word or text (case-insensitive) and get path:line: text for every match. Use this to find functions, names or errors before reading whole files.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Text to search for" },
                        "path": { "type": "string", "description": "Folder or file to search in, relative to the shared folder. Default ." },
                        "regex": { "type": "boolean", "description": "Treat query as a regular expression. Default false." }
                    },
                    "required": ["query"]
                }
            }
        }),
    ];
    if folders.iter().any(|folder| folder.writable) {
        list.push(json!({
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Change part of a text file inside a shared folder that allows writing: old_text is replaced by new_text. old_text must be copied exactly from the file (with indentation) and must appear only once; add surrounding lines to make it unique. Prefer this over write_file for changes in existing files.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path, relative to the shared folder" },
                        "old_text": { "type": "string", "description": "The exact text to replace" },
                        "new_text": { "type": "string", "description": "The new text" }
                    },
                    "required": ["path", "old_text", "new_text"]
                }
            }
        }));
        list.push(json!({
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Create or overwrite a text file inside a shared folder that allows writing. Always send the complete file content.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "File path, relative to the shared folder" },
                        "content": { "type": "string", "description": "The complete new content of the file" }
                    },
                    "required": ["path", "content"]
                }
            }
        }));
    }
    list
}

pub fn system_note(folders: &[Folder]) -> String {
    let lines = folders
        .iter()
        .map(|folder| {
            format!(
                "- {} ({})",
                folder.path,
                if folder.writable { "read and write" } else { "read only" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let documents = if folders.iter().any(|folder| folder.writable) {
        " When the user wants a PDF, Word file, slides or e-book, write it as Markdown and call create_document."
    } else {
        ""
    };
    format!(
        "The user shared these folders with you:\n{lines}\nUse list_folder, search_files and read_file to look at files before you answer questions about them; search first instead of reading many whole files. To change an existing file use edit_file with a small unique old_text. Paths are relative to the first folder. Never guess file contents.{documents}"
    )
}

pub fn run(name: &str, arguments: &Value, folders: &[Folder]) -> Option<String> {
    let path = arguments["path"].as_str().unwrap_or(".");
    let result = match name {
        "list_folder" => list_folder(folders, path, number(arguments, "depth").unwrap_or(2)),
        "read_file" => read_file(folders, path, number(arguments, "start_line").unwrap_or(1)),
        "write_file" => write_file(
            folders,
            path,
            arguments["content"].as_str().unwrap_or_default(),
        ),
        "search_files" => search_files(
            folders,
            path,
            arguments["query"].as_str().unwrap_or_default(),
            arguments["regex"].as_bool().unwrap_or(false),
        ),
        "edit_file" => edit_file(
            folders,
            path,
            arguments["old_text"].as_str().unwrap_or_default(),
            arguments["new_text"].as_str().unwrap_or_default(),
        ),
        _ => return None,
    };
    Some(result.unwrap_or_else(|e| format!("Error: {e}")))
}

fn number(arguments: &Value, key: &str) -> Option<usize> {
    let value = &arguments[key];
    value
        .as_u64()
        .or_else(|| value.as_f64().map(|n| n.max(0.0) as u64))
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
        .map(|n| n as usize)
}

fn list_folder(folders: &[Folder], path: &str, depth: usize) -> Result<String, String> {
    let target = sandbox::resolve(folders, path)?;
    if !target.path.is_dir() {
        return Err(format!("{} is not a folder", target.shown()));
    }
    let mut lines = vec![format!("{}/", target.shown())];
    let mut count = 0;
    walk(&target.path, &target.root, 1, depth.clamp(1, 4), &mut lines, &mut count);
    if count >= MAX_ENTRIES {
        lines.push(format!("[cut after {MAX_ENTRIES} entries, list a subfolder for more]"));
    }
    Ok(lines.join("\n"))
}

fn walk(dir: &Path, root: &Path, level: usize, depth: usize, lines: &mut Vec<String>, count: &mut usize) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|entry| (!entry.path().is_dir(), entry.file_name()));
    let indent = "  ".repeat(level);
    for entry in entries {
        if *count >= MAX_ENTRIES {
            return;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            let inside = entry
                .path()
                .canonicalize()
                .is_ok_and(|target| target.starts_with(root));
            if !inside {
                continue;
            }
        }
        *count += 1;
        let path = entry.path();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_str()) {
                lines.push(format!("{indent}{name}/ (skipped)"));
            } else if level < depth {
                lines.push(format!("{indent}{name}/"));
                walk(&path, root, level + 1, depth, lines, count);
            } else {
                lines.push(format!("{indent}{name}/ …"));
            }
        } else {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            lines.push(format!("{indent}{name} ({})", size_text(size)));
        }
    }
}

fn size_text(bytes: u64) -> String {
    match bytes {
        0..1_000 => format!("{bytes} B"),
        1_000..1_000_000 => format!("{:.1} KB", bytes as f64 / 1_000.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_000_000.0),
    }
}

fn read_file(folders: &[Folder], path: &str, start_line: usize) -> Result<String, String> {
    let target = sandbox::resolve(folders, path)?;
    if target.path.is_dir() {
        return Err(format!("{} is a folder, use list_folder", target.shown()));
    }
    let size = fs::metadata(&target.path)
        .map_err(|e| format!("{}: {e}", target.shown()))?
        .len();
    if size > MAX_READ_BYTES {
        return Err(format!("{} is too big ({})", target.shown(), size_text(size)));
    }
    let bytes = fs::read(&target.path).map_err(|e| format!("{}: {e}", target.shown()))?;
    if bytes.iter().take(8000).any(|&b| b == 0) {
        return Ok(format!("{} is a binary file ({})", target.shown(), size_text(size)));
    }
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<&str> = text.lines().collect();
    if all.is_empty() {
        return Ok(format!("{} is empty", target.shown()));
    }
    let start = start_line.max(1).min(all.len());
    let end = (start + MAX_LINES - 1).min(all.len());
    let width = end.to_string().len();
    let mut output = vec![format!("{} (lines {start}-{end} of {})", target.shown(), all.len())];
    for (index, line) in all[start - 1..end].iter().enumerate() {
        let line: String = if line.chars().count() > MAX_LINE_CHARS {
            line.chars().take(MAX_LINE_CHARS).collect::<String>() + " …"
        } else {
            line.to_string()
        };
        output.push(format!("{:>width$}| {line}", start + index));
    }
    if end < all.len() {
        output.push(format!("[more lines, call read_file with start_line {}]", end + 1));
    }
    Ok(output.join("\n"))
}

fn write_file(folders: &[Folder], path: &str, content: &str) -> Result<String, String> {
    let target = writable_target(folders, path)?;
    if target.path.is_dir() {
        return Err(format!("{} is a folder", target.shown()));
    }
    if content.len() > MAX_WRITE_BYTES {
        return Err("Content is too big (max. 1 MB)".to_string());
    }
    let existed = target.path.exists();
    if let Some(parent) = target.path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", target.shown()))?;
    }
    fs::write(&target.path, content).map_err(|e| format!("{}: {e}", target.shown()))?;
    Ok(format!(
        "{} {} ({} lines)",
        if existed { "Overwrote" } else { "Created" },
        target.shown(),
        content.lines().count()
    ))
}

fn writable_target(folders: &[Folder], path: &str) -> Result<sandbox::Resolved, String> {
    let target = sandbox::resolve(folders, path)?;
    if !target.writable {
        return Err(format!("{} is read only, the user did not allow writing", target.shown()));
    }
    if target
        .path
        .strip_prefix(&target.root)
        .is_ok_and(|rest| rest.components().any(|part| part.as_os_str() == ".git"))
    {
        return Err("Writing inside .git is not allowed".to_string());
    }
    Ok(target)
}

fn edit_file(folders: &[Folder], path: &str, old_text: &str, new_text: &str) -> Result<String, String> {
    let target = writable_target(folders, path)?;
    if old_text.is_empty() {
        return Err("old_text is empty".to_string());
    }
    let content = fs::read_to_string(&target.path).map_err(|e| format!("{}: {e}", target.shown()))?;
    let found = content.matches(old_text).count();
    let (content, found) = if found == 0 && content.contains("\r\n") {
        let crlf = old_text.replace("\r\n", "\n").replace('\n', "\r\n");
        let count = content.matches(crlf.as_str()).count();
        (content.replacen(&crlf, &new_text.replace("\r\n", "\n").replace('\n', "\r\n"), 1), count)
    } else {
        (content.replacen(old_text, new_text, 1), found)
    };
    match found {
        0 => Err(format!("old_text was not found in {}. Read the file again and copy the text exactly.", target.shown())),
        1 => {
            if content.len() > MAX_WRITE_BYTES {
                return Err("Content is too big (max. 1 MB)".to_string());
            }
            fs::write(&target.path, &content).map_err(|e| format!("{}: {e}", target.shown()))?;
            Ok(format!(
                "Edited {}: replaced {} lines with {} lines",
                target.shown(),
                old_text.lines().count().max(1),
                new_text.lines().count()
            ))
        }
        n => Err(format!("old_text appears {n} times in {}. Add more surrounding lines so it is unique.", target.shown())),
    }
}

fn search_files(folders: &[Folder], path: &str, query: &str, regex: bool) -> Result<String, String> {
    if query.trim().is_empty() {
        return Err("query is empty".to_string());
    }
    let pattern = if regex {
        regex::RegexBuilder::new(query).case_insensitive(true).build().map_err(|e| e.to_string())?
    } else {
        regex::RegexBuilder::new(&regex::escape(query)).case_insensitive(true).build().map_err(|e| e.to_string())?
    };
    let target = sandbox::resolve(folders, path)?;
    let mut files = Vec::new();
    collect_files(&target.path, &mut files);
    let mut lines = Vec::new();
    let mut total = 0;
    for file in &files {
        let Ok(meta) = fs::metadata(file) else { continue };
        if meta.len() > MAX_READ_BYTES {
            continue;
        }
        let Ok(bytes) = fs::read(file) else { continue };
        if bytes.iter().take(8000).any(|byte| *byte == 0) {
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let shown = file
            .strip_prefix(&target.root)
            .map(|rest| rest.display().to_string())
            .unwrap_or_else(|_| file.display().to_string());
        for (number, line) in text.lines().enumerate() {
            if pattern.is_match(line) {
                total += 1;
                if lines.len() < MAX_MATCHES {
                    let short: String = line.trim().chars().take(200).collect();
                    lines.push(format!("{shown}:{}: {short}", number + 1));
                }
            }
        }
    }
    if total == 0 {
        return Ok(format!("No matches for \"{query}\" in {} ({} files searched)", target.shown(), files.len()));
    }
    let mut out = format!("{total} matches for \"{query}\" in {}:\n{}", target.shown(), lines.join("\n"));
    if total > MAX_MATCHES {
        out.push_str(&format!("\n[only the first {MAX_MATCHES} shown, search more exactly]"));
    }
    Ok(out)
}

fn collect_files(path: &Path, out: &mut Vec<std::path::PathBuf>) {
    if out.len() >= MAX_SEARCH_FILES {
        return;
    }
    if path.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(path) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if !SKIP_DIRS.iter().any(|skip| name == *skip) {
                collect_files(&entry.path(), out);
            }
        } else if kind.is_file() && out.len() < MAX_SEARCH_FILES {
            out.push(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_read_write() {
        let base = std::env::temp_dir().join(format!("dlc-files-{}", std::process::id()));
        fs::create_dir_all(base.join("src")).unwrap();
        fs::create_dir_all(base.join("target/debug")).unwrap();
        fs::write(base.join("src/main.rs"), "fn main() {\n    println!(\"hi\");\n}\n").unwrap();
        fs::write(base.join("logo.png"), [0u8, 1, 2, 0]).unwrap();
        let root = base.canonicalize().unwrap().to_string_lossy().to_string();
        let mut folders = vec![Folder { path: root, writable: false }];

        let tree = run("list_folder", &json!({"path": "."}), &folders).unwrap();
        assert!(tree.contains("src/\n    main.rs"), "{tree}");
        assert!(tree.contains("target/ (skipped)"), "{tree}");
        assert!(!tree.contains("debug"), "{tree}");

        let file = run("read_file", &json!({"path": "src/main.rs"}), &folders).unwrap();
        assert!(file.contains("lines 1-3 of 3"), "{file}");
        assert!(file.contains("2|     println!"), "{file}");
        let binary = run("read_file", &json!({"path": "logo.png"}), &folders).unwrap();
        assert!(binary.contains("binary"), "{binary}");

        let denied = run("write_file", &json!({"path": "a.txt", "content": "x"}), &folders).unwrap();
        assert!(denied.starts_with("Error"), "{denied}");
        assert!(!base.join("a.txt").exists());
        folders[0].writable = true;
        let found = run("search_files", &json!({"query": "PRINTLN"}), &folders).unwrap();
        assert!(found.contains("src/main.rs:2:"), "{found}");
        assert_eq!(definitions(&folders).len(), 5);
        let created = run("write_file", &json!({"path": "notes/a.txt", "content": "x\ny"}), &folders).unwrap();
        assert!(created.starts_with("Created"), "{created}");
        assert_eq!(fs::read_to_string(base.join("notes/a.txt")).unwrap(), "x\ny");
        fs::write(base.join("src/lib.rs"), "fn a() {\n    1\n}\nfn b() {\n    1\n}\n").unwrap();
        let twice = run("edit_file", &json!({"path": "src/lib.rs", "old_text": "    1", "new_text": "    2"}), &folders).unwrap();
        assert!(twice.contains("2 times"), "{twice}");
        let edited = run("edit_file", &json!({"path": "src/lib.rs", "old_text": "fn b() {\n    1", "new_text": "fn b() {\n    2"}), &folders).unwrap();
        assert!(edited.starts_with("Edited"), "{edited}");
        assert!(fs::read_to_string(base.join("src/lib.rs")).unwrap().ends_with("fn b() {\n    2\n}\n"));
        let escape = run("write_file", &json!({"path": "../evil.txt", "content": "x"}), &folders).unwrap();
        assert!(escape.starts_with("Error"), "{escape}");
        let _ = fs::remove_dir_all(&base);
    }
}
