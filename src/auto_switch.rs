use crate::hardware;
use crate::ollama::{self, InstalledModel};

pub const ROUTER_KEEP_ALIVE: &str = "30m";
pub const THINK_AUTO: i32 = 0;
pub const THINK_ON: i32 = 1;
pub const THINK_OFF: i32 = 2;

pub fn think_off(mode: i32, task: Task) -> bool {
    match mode {
        THINK_ON => false,
        THINK_OFF => true,
        _ => task == Task::Simple,
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Task {
    Simple,
    Coding,
    Complex,
}

impl Task {
    pub fn label(self) -> &'static str {
        match self {
            Task::Simple => "simple chat",
            Task::Coding => "coding",
            Task::Complex => "complex task",
        }
    }
}

pub struct Choice {
    pub model: String,
    pub task: Task,
    pub kept: bool,
}

impl Choice {
    pub fn note(&self) -> String {
        if self.kept {
            format!("Auto-Switch (beta): {} → kept {}", self.task.label(), self.model)
        } else {
            format!("Auto-Switch (beta): {} → switched to {}", self.task.label(), self.model)
        }
    }
}

const CODE_HINTS: [&str; 22] = [
    "```", "code", "function", "funktion", "bug", "error", "fehler", "compile", "rust", "python", "javascript",
    "typescript", "sql", "regex", "script", "class ", "struct", "api", "programm", "refactor", "debug", "cargo",
];

const COMPLEX_HINTS: [&str; 10] = [
    "architect", "architektur", "design a", "entwirf", "plan ", "compare", "vergleich", "explain why", "analys", "strategy",
];

pub fn guess(prompt: &str) -> Task {
    let lower = prompt.to_lowercase();
    let code = CODE_HINTS.iter().filter(|hint| lower.contains(*hint)).count();
    let complex = COMPLEX_HINTS.iter().any(|hint| lower.contains(hint));
    if code >= 2 || lower.contains("```") {
        Task::Coding
    } else if complex || code == 1 && lower.len() > 300 || lower.len() > 1200 {
        Task::Complex
    } else {
        Task::Simple
    }
}

pub fn parse_answer(answer: &str) -> Option<Task> {
    let lower = answer.trim().to_lowercase();
    if lower.contains("coding") || lower.contains("code") {
        Some(Task::Coding)
    } else if lower.contains("complex") {
        Some(Task::Complex)
    } else if lower.contains("simple") {
        Some(Task::Simple)
    } else {
        None
    }
}

fn is_chat_model(model: &InstalledModel) -> bool {
    let name = model.name.to_lowercase();
    !["embed", "whisper", "bge", "minilm", "rerank"]
        .iter()
        .any(|word| name.contains(word))
}

fn is_coder(name: &str) -> bool {
    let name = name.to_lowercase();
    name.contains("coder") || name.contains("code") || name.contains("devstral") || name.contains("starcoder")
}

fn fits(model: &InstalledModel) -> bool {
    hardware::get().estimate(model.size).level <= 1
}

pub fn router_model(models: &[InstalledModel]) -> Option<&InstalledModel> {
    models
        .iter()
        .filter(|model| is_chat_model(model) && model.size > 0)
        .min_by_key(|model| model.size)
}

pub fn pick<'a>(task: Task, models: &'a [InstalledModel], loaded: &[String]) -> Option<&'a InstalledModel> {
    let usable: Vec<&InstalledModel> = models.iter().filter(|model| is_chat_model(model)).collect();
    let fitting: Vec<&InstalledModel> = usable.iter().copied().filter(|model| fits(model)).collect();
    let pool = if fitting.is_empty() { usable } else { fitting };
    if pool.is_empty() {
        return None;
    }
    let largest = pool.iter().map(|model| model.size).max().unwrap_or(0);
    let candidates: Vec<&InstalledModel> = match task {
        Task::Simple => {
            let mut sorted = pool.clone();
            sorted.sort_by_key(|model| model.size);
            let middle = sorted[sorted.len() / 2].size;
            let small: Vec<&InstalledModel> = sorted.into_iter().filter(|model| model.size <= middle).collect();
            let general: Vec<&InstalledModel> = small.iter().copied().filter(|model| !is_coder(&model.name)).collect();
            if general.is_empty() { small } else { general }
        }
        Task::Coding => {
            let coders: Vec<&InstalledModel> = pool.iter().copied().filter(|model| is_coder(&model.name)).collect();
            if coders.is_empty() {
                pool.iter().copied().filter(|model| model.size * 2 >= largest).collect()
            } else {
                coders
            }
        }
        Task::Complex => pool.iter().copied().filter(|model| model.size * 2 >= largest).collect(),
    };
    if let Some(model) = candidates.iter().find(|model| loaded.contains(&model.name)) {
        return Some(model);
    }
    if task == Task::Simple {
        let useful = candidates.iter().copied().filter(|model| model.size >= 2 << 30).min_by_key(|model| model.size);
        return useful.or_else(|| candidates.into_iter().max_by_key(|model| model.size));
    }
    candidates.into_iter().max_by_key(|model| model.size)
}

const ROUTER_PROMPT: &str = "Classify the user request below. Answer with exactly one word:\nsimple = small talk, short questions, everyday facts, translations\ncoding = writing, fixing or explaining program code, commands or configs\ncomplex = architecture, planning, long analysis, hard reasoning\n\nRequest:\n";

pub fn choose(prompt: &str, current: &str) -> Option<Choice> {
    let models = ollama::list_models().ok()?;
    let loaded: Vec<String> = ollama::running_models()
        .map(|list| list.into_iter().map(|model| model.name).collect())
        .unwrap_or_default();
    let short: String = prompt.chars().take(1500).collect();
    let task = router_model(&models)
        .and_then(|router| ollama::classify(&router.name, &format!("{ROUTER_PROMPT}{short}"), ROUTER_KEEP_ALIVE).ok())
        .and_then(|answer| parse_answer(&answer))
        .unwrap_or_else(|| guess(prompt));
    let chosen = pick(task, &models, &loaded)?;
    let current_ok = models.iter().any(|model| model.name == current)
        && pick_contains(task, &models, &loaded, current);
    let model = if current_ok { current.to_string() } else { chosen.name.clone() };
    Some(Choice {
        kept: model == current,
        model,
        task,
    })
}

fn pick_contains(task: Task, models: &[InstalledModel], loaded: &[String], name: &str) -> bool {
    let mut with_current = loaded.to_vec();
    with_current.retain(|loaded_name| loaded_name != name);
    with_current.insert(0, name.to_string());
    pick(task, models, &with_current).is_some_and(|model| model.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str, gib: u64) -> InstalledModel {
        InstalledModel {
            name: name.to_string(),
            size: gib << 30,
            parameter_size: String::new(),
            quantization: String::new(),
        }
    }

    #[test]
    fn guesses_and_answers() {
        assert_eq!(guess("hi, wie geht's?"), Task::Simple);
        assert_eq!(guess("fix this rust function, it has a compile error"), Task::Coding);
        assert_eq!(guess("Entwirf die Architektur für meinen Server"), Task::Complex);
        assert_eq!(parse_answer(" Coding."), Some(Task::Coding));
        assert_eq!(parse_answer("simple"), Some(Task::Simple));
        assert_eq!(parse_answer("??"), None);
    }

    #[test]
    fn think_modes() {
        assert!(think_off(THINK_AUTO, Task::Simple));
        assert!(!think_off(THINK_AUTO, Task::Coding));
        assert!(!think_off(THINK_ON, Task::Simple));
        assert!(think_off(THINK_OFF, Task::Complex));
    }

    #[test]
    fn picks_by_task_and_prefers_loaded() {
        let models = vec![model("gemma3:1b", 1), model("qwen2.5-coder:7b", 5), model("qwen3:8b", 5), model("nomic-embed-text", 1)];
        assert_eq!(router_model(&models).unwrap().name, "gemma3:1b");
        assert_eq!(pick(Task::Coding, &models, &[]).unwrap().name, "qwen2.5-coder:7b");
        assert_eq!(pick(Task::Simple, &models, &[]).unwrap().name, "qwen3:8b");
        let loaded = vec!["gemma3:1b".to_string()];
        assert_eq!(pick(Task::Simple, &models, &loaded).unwrap().name, "gemma3:1b");
        let loaded = vec!["qwen3:8b".to_string()];
        assert_eq!(pick(Task::Complex, &models, &loaded).unwrap().name, "qwen3:8b");
    }
}
