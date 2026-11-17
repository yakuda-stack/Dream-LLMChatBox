pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn answer(args: &[String]) -> Option<String> {
    match args.get(1).map(String::as_str) {
        Some("--version" | "-V") => Some(format!("dream-llmchatbox {VERSION}")),
        Some("--help" | "-h") => Some(format!(
            "Dream-LLMChatBox {VERSION}\nChat with local AI models (Ollama).\n\nUsage: dream-llmchatbox [--version | --help]\nWithout arguments the app window opens."
        )),
        _ => None,
    }
}

pub fn handled() -> bool {
    let args: Vec<String> = std::env::args().collect();
    match answer(&args) {
        Some(text) => {
            println!("{text}");
            true
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_and_help() {
        let args = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(answer(&args(&["x", "--version"])), Some(format!("dream-llmchatbox {VERSION}")));
        assert!(answer(&args(&["x", "-h"])).unwrap().contains("Usage"));
        assert_eq!(answer(&args(&["x"])), None);
    }
}
