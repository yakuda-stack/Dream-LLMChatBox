use crate::i18n::{tr, trf};
use crate::server;
use crate::settings;
use std::process::Command;

const DROP_IN_DIR: &str = "/etc/systemd/system/ollama.service.d";
const DROP_IN: &str = "/etc/systemd/system/ollama.service.d/dream-llmchatbox-backend.conf";

pub fn env(choice: u32) -> Vec<(&'static str, &'static str)> {
    match choice {
        1 => vec![("OLLAMA_VULKAN", "1"), ("HIP_VISIBLE_DEVICES", "-1"), ("ROCR_VISIBLE_DEVICES", "-1")],
        2 => vec![("OLLAMA_VULKAN", "0"), ("GGML_VK_VISIBLE_DEVICES", "-1")],
        3 => vec![
            ("OLLAMA_VULKAN", "0"),
            ("GGML_VK_VISIBLE_DEVICES", "-1"),
            ("HIP_VISIBLE_DEVICES", "-1"),
            ("ROCR_VISIBLE_DEVICES", "-1"),
            ("CUDA_VISIBLE_DEVICES", "-1"),
        ],
        _ => Vec::new(),
    }
}

pub fn drop_in_text(choice: u32) -> String {
    let mut text = String::from("[Service]\n");
    for (key, value) in env(choice) {
        text.push_str(&format!("Environment=\"{key}={value}\"\n"));
    }
    text
}

pub fn service_script(choice: u32, restart: bool) -> String {
    let write = if choice == 0 {
        format!("rm -f {DROP_IN}")
    } else {
        format!(
            "mkdir -p {DROP_IN_DIR} && printf '%s' '{}' > {DROP_IN}",
            drop_in_text(choice)
        )
    };
    let restart = if restart { " && systemctl restart ollama.service" } else { "" };
    format!("{write} && systemctl daemon-reload{restart}")
}

pub fn apply(choice: u32) -> Result<String, String> {
    settings::update(|s| s.ollama_backend = choice);
    match server::systemd_service() {
        Some(active) => {
            let status = Command::new("pkexec")
                .args(["sh", "-c", &service_script(choice, active)])
                .status()
                .map_err(|e| format!("pkexec: {e}"))?;
            if !status.success() {
                return Err(tr("Authorization was cancelled or failed"));
            }
            Ok(tr("Saved for the Ollama system service and restarted Ollama."))
        }
        None => {
            if server::is_own_running() {
                server::stop_own();
                server::start()?;
                return Ok(tr("Ollama was restarted with the new backend."));
            }
            Ok(tr("Saved – used the next time this app starts Ollama."))
        }
    }
}

pub fn detect() -> Option<String> {
    let output = Command::new("journalctl")
        .args(["-u", "ollama", "-n", "400", "--no-pager", "-o", "cat"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .rev()
        .filter_map(|line| line.split("load_backend: loaded ").nth(1))
        .map(|rest| rest.split(" backend").next().unwrap_or(rest).trim().to_string())
        .find(|name| name != "CPU")
}

pub fn status() -> String {
    match detect() {
        Some(name) => trf("Last used by Ollama: {}", &[&name]),
        None => tr("Which backend Ollama uses shows up here after a model was loaded."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_service_files() {
        assert!(env(0).is_empty());
        assert_eq!(drop_in_text(1), "[Service]\nEnvironment=\"OLLAMA_VULKAN=1\"\nEnvironment=\"HIP_VISIBLE_DEVICES=-1\"\nEnvironment=\"ROCR_VISIBLE_DEVICES=-1\"\n");
        let script = service_script(2, true);
        assert!(script.contains("OLLAMA_VULKAN=0") && script.ends_with("systemctl restart ollama.service"));
        assert!(service_script(0, false).starts_with("rm -f /etc/systemd/system/ollama.service.d/dream-llmchatbox-backend.conf"));
    }
}
