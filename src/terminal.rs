use std::process::{Child, Command};

const TERMINALS: [(&str, &[&str]); 9] = [
    ("konsole", &["-e"]),
    ("kitty", &[]),
    ("alacritty", &["-e"]),
    ("foot", &[]),
    ("wezterm", &["start", "--"]),
    ("ptyxis", &["--"]),
    ("gnome-terminal", &["--"]),
    ("xfce4-terminal", &["-x"]),
    ("xterm", &["-e"]),
];

pub fn run(script: &str) -> Result<Child, String> {
    if let Ok(terminal) = std::env::var("TERMINAL")
        && !terminal.is_empty()
        && let Ok(child) = spawn(&terminal, &["-e"], script)
    {
        return Ok(child);
    }

    for (terminal, args) in TERMINALS {
        if let Ok(child) = spawn(terminal, args, script) {
            return Ok(child);
        }
    }
    Err("No terminal found (tried konsole, kitty, alacritty, foot, …)".to_string())
}

fn spawn(terminal: &str, args: &[&str], script: &str) -> std::io::Result<Child> {
    Command::new(terminal)
        .args(args)
        .args(["sh", "-c", script])
        .spawn()
}
