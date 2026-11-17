#[derive(Debug, PartialEq)]
pub enum Kind {
    Text,
    Heading,
    Code,
    Tool,
    Think,
    Bullet,
}

pub const TOOL_FENCE: &str = "```dream-tool";
pub const THINK_FENCE: &str = "```dream-think";

pub struct Segment {
    pub kind: Kind,
    pub language: String,
    pub text: String,
}

const PROSE_LANGUAGES: [&str; 4] = ["markdown", "md", "text", "txt"];

pub fn split(text: &str) -> Vec<Segment> {
    let mut segments: Vec<Segment> = Vec::new();
    let mut prose = String::new();
    let mut code: Option<(String, String)> = None;

    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        match code.as_mut() {
            None if trimmed.starts_with("```") => {
                push_prose(&mut segments, &mut prose);
                let language = trimmed.trim_start_matches('`').trim().to_lowercase();
                code = Some((language, String::new()));
            }
            None => prose.push_str(line),
            Some(_) if trimmed == "```" => {
                if let Some((language, body)) = code.take() {
                    push_code(&mut segments, language, body);
                }
            }
            Some((_, body)) => body.push_str(line),
        }
    }

    match code {
        Some((language, body)) => push_code(&mut segments, language, body),
        None => push_prose(&mut segments, &mut prose),
    }
    segments
}

fn push_code(segments: &mut Vec<Segment>, language: String, body: String) {
    if PROSE_LANGUAGES.contains(&language.as_str()) {
        let mut prose = body;
        push_prose(segments, &mut prose);
        return;
    }
    let text = body.trim_end_matches('\n').to_string();
    if text.trim().is_empty() {
        return;
    }
    let kind = if language == "dream-tool" {
        Kind::Tool
    } else if language == "dream-think" || language == crate::compact::LANGUAGE {
        Kind::Think
    } else {
        Kind::Code
    };
    segments.push(Segment {
        kind,
        language,
        text,
    });
}

fn push_prose(segments: &mut Vec<Segment>, prose: &mut String) {
    let mut paragraph = String::new();
    for line in prose.lines() {
        let trimmed = line.trim_start();
        let hashes = trimmed.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ') {
            flush_text(segments, &mut paragraph);
            segments.push(Segment {
                kind: Kind::Heading,
                language: String::new(),
                text: clean_inline(trimmed[hashes..].trim()),
            });
            continue;
        }
        let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.len() >= 3 && compact.chars().all(|c| c == '-' || c == '*' || c == '_') {
            flush_text(segments, &mut paragraph);
            continue;
        }
        if let Some(item) = bullet_item(trimmed) {
            flush_text(segments, &mut paragraph);
            let indent = line.len() - trimmed.len();
            segments.push(Segment {
                kind: Kind::Bullet,
                language: (indent / 2).min(4).to_string(),
                text: clean_inline(item.trim()),
            });
            continue;
        }
        paragraph.push_str(&clean_line(line));
        paragraph.push('\n');
    }
    flush_text(segments, &mut paragraph);
    prose.clear();
}

fn flush_text(segments: &mut Vec<Segment>, paragraph: &mut String) {
    let text = paragraph.trim_matches('\n').to_string();
    paragraph.clear();
    if !text.trim().is_empty() {
        segments.push(Segment {
            kind: Kind::Text,
            language: String::new(),
            text,
        });
    }
}

fn bullet_item(trimmed: &str) -> Option<&str> {
    trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .or_else(|| trimmed.strip_prefix("• "))
}

fn clean_line(line: &str) -> String {
    let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    let rest = &line[indent.len()..];
    let rest = rest
        .strip_prefix("- ")
        .or_else(|| rest.strip_prefix("* "))
        .or_else(|| rest.strip_prefix("+ "))
        .map(|item| format!("• {item}"))
        .unwrap_or_else(|| rest.to_string());
    format!("{indent}{}", clean_inline(&rest))
}

fn clean_inline(text: &str) -> String {
    text.replace("**", "").replace("__", "")
}

pub fn inside_code(text: &str) -> bool {
    text.lines()
        .filter(|line| line.trim_start().starts_with("```"))
        .count()
        % 2
        == 1
}

pub fn tool_block(call: &str, result: &str) -> String {
    let result: String = result.chars().take(600).collect();
    format!("\n{TOOL_FENCE}\n{call}\n→ {result}\n```\n")
}

pub fn without_tool_blocks(text: &str) -> String {
    let mut output = String::new();
    let mut inside = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if !inside && (trimmed == TOOL_FENCE || trimmed == THINK_FENCE) {
            inside = true;
        } else if inside && trimmed == "```" {
            inside = false;
        } else if !inside {
            output.push_str(line);
        }
    }
    output.trim().to_string()
}

pub fn without_code(text: &str) -> String {
    split(text)
        .into_iter()
        .filter(|segment| matches!(segment.kind, Kind::Text | Kind::Heading | Kind::Bullet))
        .map(|segment| segment.text.replace('•', ""))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_code_blocks() {
        let text = "Hier:\n```rust\nfn main() {}\n```\nFertig.";
        let segments = split(text);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].text, "Hier:");
        assert_eq!(segments[1].kind, Kind::Code);
        assert_eq!(segments[1].language, "rust");
        assert_eq!(segments[1].text, "fn main() {}");
        assert_eq!(segments[2].text, "Fertig.");
    }

    #[test]
    fn open_block_while_streaming() {
        let text = "Code:\n```python\nprint(1)\n";
        let segments = split(text);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1].kind, Kind::Code);
        assert_eq!(segments[1].text, "print(1)");
        assert!(inside_code(text));
        assert!(!inside_code("a\n```\nb\n```\n"));
    }

    #[test]
    fn empty_and_markdown_blocks() {
        let segments = split("Ende.\n```\n");
        assert_eq!(segments.len(), 1);
        let segments = split("```markdown\n### Titel\nText **fett**\n```");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].kind, Kind::Heading);
        assert_eq!(segments[0].text, "Titel");
        assert_eq!(segments[1].text, "Text fett");
    }

    #[test]
    fn cleans_markdown() {
        let segments = split("### **5. Warum**\n- **Punkt**:\n  - unter\n---\nSchluss");
        assert_eq!(segments[0].kind, Kind::Heading);
        assert_eq!(segments[0].text, "5. Warum");
        assert_eq!(segments[1].kind, Kind::Bullet);
        assert_eq!(segments[1].text, "Punkt:");
        assert_eq!((segments[2].text.as_str(), segments[2].language.as_str()), ("unter", "1"));
        assert_eq!(segments[3].text, "Schluss");
        let plus = split("Liste:\n+ eins\n- zwei");
        assert_eq!(plus.iter().filter(|s| s.kind == Kind::Bullet).count(), 2);
        assert_eq!(without_code("A\n```rust\nx\n```\nB"), "A\nB");
    }

    #[test]
    fn tool_blocks() {
        let text = format!("Moment.{}Es ist 42.", tool_block("calculate(expression: 6*7)", "42"));
        let segments = split(&text);
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[1].kind, Kind::Tool);
        assert_eq!(segments[1].text, "calculate(expression: 6*7)\n→ 42");
        assert_eq!(without_tool_blocks(&text), "Moment.\nEs ist 42.");
        assert_eq!(without_code(&text), "Moment.\nEs ist 42.");
    }
}
