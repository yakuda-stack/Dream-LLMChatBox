use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const CODING: &str = "coding";
pub const HAS_ARCHITECTURE: u8 = 1;
pub const NO_ARCHITECTURE: u8 = 2;
pub const NEW_PROJECT: u8 = 3;
const ARCHITECTURE_NAMES: [&str; 2] = ["architektur.md", "architecture.md"];
const MAX_ARCHITECTURE_CHARS: usize = 12_000;
const MAX_ROADMAP_CHARS: usize = 6_000;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Mode {
    #[serde(default)]
    pub purpose: String,
    #[serde(default)]
    pub coding: u8,
}

impl Mode {
    pub fn is_coding(&self) -> bool {
        self.purpose == CODING
    }

    pub fn to_json(&self) -> String {
        if *self == Mode::default() {
            String::new()
        } else {
            serde_json::to_string(self).unwrap_or_default()
        }
    }

    pub fn from_json(text: &str) -> Mode {
        serde_json::from_str(text).unwrap_or_default()
    }

    pub fn choice(&self, found: bool) -> u8 {
        match self.coding {
            HAS_ARCHITECTURE | NO_ARCHITECTURE | NEW_PROJECT => self.coding,
            _ if found => HAS_ARCHITECTURE,
            _ => NO_ARCHITECTURE,
        }
    }
}

pub struct Project<'a> {
    pub root: &'a Path,
    pub tools: bool,
    pub writable: bool,
    pub memory: bool,
}

fn find_in(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            names.contains(&name.as_str())
        })
        .map(|entry| entry.path())
        .collect();
    found.sort_by_key(|path| {
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        names.iter().position(|wanted| *wanted == name)
    });
    found.into_iter().next()
}

pub fn find_architecture(root: &Path) -> Option<PathBuf> {
    find_in(root, &ARCHITECTURE_NAMES).or_else(|| find_in(&root.join("docs"), &ARCHITECTURE_NAMES))
}

pub fn find_roadmap(root: &Path) -> Option<PathBuf> {
    find_in(root, &["roadmap.md"])
}

pub fn architecture_hint(root: &Path, writable: bool) -> String {
    let Some(path) = find_architecture(root) else {
        return "The project has no ARCHITEKTUR.md. If you had to look through many files to answer, offer the user at the end to write a short ARCHITEKTUR.md (structure, modules, data flow) so later chats need fewer tool calls.".to_string();
    };
    let name = relative(root, &path);
    let update = if writable {
        format!("If your work changes the structure (new, moved or removed files or modules, new data flow), update {name} with edit_file in the same answer – only the parts that changed.")
    } else {
        format!("The folder is read only: if your work changes the structure, tell the user in one short list what to add to {name}.")
    };
    let index = crate::projects::architecture_index(root)
        .map(|(_, index)| format!("\n\nIndex of {name} (headings and modules – open the file only for details):\n{index}"))
        .unwrap_or_default();
    format!(
        "The project has {name} ({} lines), a map of its structure. Use the index below to find the right files; read {name} with read_file only when you need more detail, before you call list_folder or search_files. Search the whole project only if {name} does not answer the question. {update}{index}",
        line_count(&path)
    )
}

pub fn line_count(path: &Path) -> usize {
    fs::read_to_string(path).map(|text| text.lines().count()).unwrap_or(0)
}

pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

fn cut(text: &str, max: usize, name: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let short: String = text.chars().take(max).collect();
    format!("{short}\n[cut after {max} characters – open {name} with read_file for the rest]")
}

const TEMPLATE: &str = "# <Project> – Architektur
## Zweck
## Aufbau / Schichten
## Dateien und Aufgaben
| Datei | Aufgabe |
|---|---|
## Datenfluss
## Bauen & Starten
## Konventionen";

const SELF_CHECK: &str = "\n\nBefore you say a change is done, check it against this list and fix what is missing:
- What happens right after the action? Does the UI or the state refresh?
- Does it work on every system and variant the project supports?
- Are new user texts in every language file?
- What does the user see when it fails (error message, status)?
- Is the code in the file the architecture names for this job?";

const NEW_QUESTIONS: &str = "\n\nThis is a brand new project. First ask the user at most 5 short questions: the goal, the language/framework, the platform, the main features and anything special. ";

fn write_steps(project: Option<&Project>) -> String {
    let mut text = format!(
        "Then write ARCHITEKTUR.md in the project root with exactly this template (fill every section, keep it short):\n{TEMPLATE}\n"
    );
    let Some(project) = project else {
        text.push_str("No project folder is shared, so show ARCHITEKTUR.md in the chat so the user can save it. Then do the user's task.");
        return text;
    };
    if !project.tools {
        text.push_str("You can't save files here, so show ARCHITEKTUR.md in the chat so the user can save it. ");
    } else if project.writable {
        text.push_str("Save it with write_file. ");
    } else {
        text.push_str(
            "The folder is read only, so you can't save files: show ARCHITEKTUR.md in the chat and ask the user to allow writing (click 'read only' on the folder chip) or to save it. ",
        );
    }
    text.push_str("After that, show a short summary of the architecture in the chat");
    if project.memory {
        text.push_str(" and save a short summary with memory_write as \"architecture.md\"");
    }
    text.push_str(". Then do the user's task.");
    text
}

pub fn system_note(mode: &Mode, project: Option<&Project>) -> Option<String> {
    if !mode.is_coding() {
        return None;
    }
    let mut note = String::from("This is a coding chat. Keep token use low: work from a short project map instead of reading many files.");
    let Some(project) = project else {
        if mode.coding == NEW_PROJECT {
            note.push_str(NEW_QUESTIONS);
            note.push_str(&write_steps(None));
        } else {
            note.push_str(" No project folder is shared, so ask the user to paste the important files when you need them.");
        }
        note.push_str("\nAnswer in the user's language.");
        return Some(note);
    };
    let architecture = find_architecture(project.root);
    match (mode.choice(architecture.is_some()), architecture) {
        (_, Some(path)) => {
            let name = relative(project.root, &path);
            let content = fs::read_to_string(&path).unwrap_or_default();
            note.push_str(&format!(
                "\n\nProject architecture (from {name}):\n{}\n\nRead this first and work from it; open other files only when the task needs them. When you change the structure, tell the user and offer to update {name}.",
                cut(&content, MAX_ARCHITECTURE_CHARS, &name)
            ));
        }
        (NEW_PROJECT, None) => {
            note.push_str(NEW_QUESTIONS);
            note.push_str(&write_steps(Some(project)));
        }
        (_, None) if project.tools => {
            note.push_str("\n\nThe project has no ARCHITEKTUR.md yet. First explore the project: call list_folder, then read only key files like Cargo.toml, package.json, pyproject.toml, README and the main entry files. ");
            note.push_str(&write_steps(Some(project)));
        }
        (_, None) => {
            note.push_str("\n\nThe project has no ARCHITEKTUR.md yet and you can't open files here. Ask the user to paste the important files (for example Cargo.toml or package.json, the README and the main entry file). Then write ARCHITEKTUR.md with exactly this template and show it in the chat so the user can save it:\n");
            note.push_str(TEMPLATE);
            note.push_str("\nThen do the user's task.");
        }
    }
    note.push_str(SELF_CHECK);
    if project.tools
        && project.writable
        && let Some(command) = crate::auto_check::command(project.root)
    {
        note.push_str(&format!(
            "\nAfter you change files, the app runs `{command}` by itself and sends you any errors or warnings. Fix them before you answer."
        ));
    }
    match find_roadmap(project.root) {
        Some(path) => {
            let name = relative(project.root, &path);
            let content = fs::read_to_string(&path).unwrap_or_default();
            note.push_str(&format!(
                "\n\nProject roadmap ({name}):\n{}\n\nWhen you finish an item, offer to tick it off.",
                cut(&content, MAX_ROADMAP_CHARS, &name)
            ));
        }
        None => note.push_str(
            "\n\nIf there is no ROADMAP.md yet, ask the user once whether you should create one with their goals as a checklist (- [ ] items). Only create it if they say yes.",
        ),
    }
    note.push_str("\nAnswer in the user's language.");
    Some(note)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dlc-coding-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn project(root: &Path, tools: bool, writable: bool) -> Project<'_> {
        Project {
            root,
            tools,
            writable,
            memory: true,
        }
    }

    fn coding(choice: u8) -> Mode {
        Mode {
            purpose: CODING.into(),
            coding: choice,
        }
    }

    #[test]
    fn mode_json_and_choice() {
        assert_eq!(Mode::default().to_json(), "");
        let mode = coding(NEW_PROJECT);
        assert_eq!(Mode::from_json(&mode.to_json()), mode);
        assert_eq!(Mode::from_json("broken"), Mode::default());
        assert_eq!(coding(0).choice(true), HAS_ARCHITECTURE);
        assert_eq!(coding(0).choice(false), NO_ARCHITECTURE);
        assert_eq!(coding(NEW_PROJECT).choice(true), NEW_PROJECT);
        assert!(system_note(&Mode::default(), None).is_none());
        assert!(system_note(&coding(0), None).unwrap().contains("paste"));
        assert!(system_note(&coding(NEW_PROJECT), None).unwrap().contains("at most 5 short questions"));
    }

    #[test]
    fn architecture_hint_points_to_file() {
        let dir = std::env::temp_dir().join(format!("dlc-hint-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        assert!(architecture_hint(&dir, true).contains("no ARCHITEKTUR.md"));
        fs::write(dir.join("ARCHITEKTUR.md"), "# A\nb\n").unwrap();
        let hint = architecture_hint(&dir, true);
        assert!(hint.contains("ARCHITEKTUR.md (2 lines)") && hint.contains("edit_file"), "{hint}");
        assert!(architecture_hint(&dir, false).contains("read only"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn finds_architecture_case_insensitive() {
        let dir = temp("find");
        assert!(find_architecture(&dir).is_none());
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs").join("Architecture.MD"), "a\nb\nc\n").unwrap();
        let found = find_architecture(&dir).unwrap();
        assert_eq!(relative(&dir, &found), "docs/Architecture.MD");
        assert_eq!(line_count(&found), 3);
        fs::write(dir.join("architektur.md"), "root").unwrap();
        assert_eq!(relative(&dir, &find_architecture(&dir).unwrap()), "architektur.md");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn note_with_architecture_and_roadmap() {
        let dir = temp("has");
        fs::write(dir.join("ARCHITEKTUR.md"), "# App – Architektur\nx".repeat(3000)).unwrap();
        fs::write(dir.join("ROADMAP.md"), "- [ ] Tests").unwrap();
        let note = system_note(&coding(HAS_ARCHITECTURE), Some(&project(&dir, true, false))).unwrap();
        assert!(note.contains("Project architecture (from ARCHITEKTUR.md)"));
        assert!(note.contains("[cut after 12000 characters"));
        assert!(note.contains("Project roadmap (ROADMAP.md):\n- [ ] Tests"));
        assert!(note.contains("offer to tick it off"));
        assert!(!note.contains("ask the user once"));
        assert!(note.ends_with("Answer in the user's language."));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn note_without_architecture() {
        let dir = temp("none");
        let note = system_note(&coding(NO_ARCHITECTURE), Some(&project(&dir, true, true))).unwrap();
        assert!(note.contains("list_folder"));
        assert!(note.contains("## Dateien und Aufgaben"));
        assert!(note.contains("write_file"));
        assert!(note.contains("memory_write as \"architecture.md\""));
        assert!(note.contains("ask the user once whether you should create one"));
        let read_only = system_note(&coding(NO_ARCHITECTURE), Some(&project(&dir, true, false))).unwrap();
        assert!(read_only.contains("read only"));
        let no_tools = system_note(&coding(NO_ARCHITECTURE), Some(&project(&dir, false, true))).unwrap();
        assert!(no_tools.contains("paste the important files"));
        assert!(!no_tools.contains("list_folder"));
        let new = system_note(&coding(NEW_PROJECT), Some(&project(&dir, true, true))).unwrap();
        assert!(new.contains("at most 5 short questions"));
        assert!(new.contains("## Bauen & Starten"));
        let _ = fs::remove_dir_all(&dir);
    }
}
