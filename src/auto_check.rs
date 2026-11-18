use std::path::Path;

pub const MAX_ROUNDS: usize = 3;
const WRITES: [&str; 3] = ["write_file", "edit_file", crate::documents::NAME];

pub fn command(root: &Path) -> Option<&'static str> {
    root.join("Cargo.toml").is_file().then_some("cargo clippy --all-targets")
}

pub fn changes_files(tool: &str, output: &str) -> bool {
    WRITES.contains(&tool) && !output.starts_with("Error") && !output.starts_with("The user did not")
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Clean,
    Problems,
    Skipped,
}

pub fn judge(output: &str) -> Outcome {
    if output.starts_with("Error") || output.starts_with("The user did not") || output.contains("[stopped") {
        return Outcome::Skipped;
    }
    let problem = output.contains("[exit code")
        || output
            .lines()
            .map(str::trim_start)
            .any(|line| line.starts_with("warning") || line.starts_with("error"));
    if problem { Outcome::Problems } else { Outcome::Clean }
}

pub fn feedback(command: &str, output: &str, round: usize) -> String {
    format!(
        "Automatic check after your file changes: `{command}` found problems (round {round} of {MAX_ROUNDS}). Fix every error and warning below with edit_file. Change only what is needed, then answer with a short summary.\n\n{output}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judges_output() {
        assert_eq!(judge("    Finished `dev` profile"), Outcome::Clean);
        assert_eq!(judge("(no output)"), Outcome::Clean);
        assert_eq!(judge("warning: unused variable: `x`\n --> src/main.rs:2:9"), Outcome::Problems);
        assert_eq!(judge("error[E0308]: mismatched types\n[exit code 101]"), Outcome::Problems);
        assert_eq!(judge("The user did not allow this command."), Outcome::Skipped);
        assert_eq!(judge("Error: blocked – x"), Outcome::Skipped);
        assert_eq!(judge("Checking x\n[stopped by the user]"), Outcome::Skipped);
    }

    #[test]
    fn detects_writes() {
        assert!(changes_files("edit_file", "Edited src/main.rs"));
        assert!(!changes_files("edit_file", "Error: text not found"));
        assert!(!changes_files("read_file", "fn main() {}"));
    }

    #[test]
    fn finds_command() {
        let dir = std::env::temp_dir().join(format!("dlc-auto-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(command(&dir), None);
        std::fs::write(dir.join("Cargo.toml"), "[package]").unwrap();
        assert_eq!(command(&dir), Some("cargo clippy --all-targets"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
