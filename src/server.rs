use crate::i18n::{tr, trf};
use crate::ollama;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

static OWN_SERVER: Mutex<Option<Child>> = Mutex::new(None);

pub fn systemd_service() -> Option<bool> {
    let known = Command::new("systemctl")
        .args(["cat", "ollama.service"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()?
        .success();
    if !known {
        return None;
    }
    let active = Command::new("systemctl")
        .args(["is-active", "--quiet", "ollama.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    Some(active)
}

fn run_privileged(args: &[&str]) -> Result<(), String> {
    let status = Command::new("pkexec")
        .args(args)
        .status()
        .map_err(|e| format!("pkexec: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(tr("Authorization was cancelled or failed"))
    }
}

pub const INSTALL_SCRIPT: &str = "curl -fsSL https://ollama.com/install.sh | sh";

pub fn is_installed() -> bool {
    crate::documents::which("ollama")
        || ["/usr/local/bin/ollama", "/usr/bin/ollama"].iter().any(|path| std::path::Path::new(path).exists())
}

pub fn start() -> Result<(), String> {
    if ollama::is_running() {
        return Ok(());
    }
    match systemd_service() {
        Some(_) => run_privileged(&["systemctl", "start", "ollama.service"])?,
        None => {
            let child = Command::new("ollama")
                .arg("serve")
                .envs(crate::ollama_backend::env(crate::settings::load().ollama_backend))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        tr("Ollama is not installed (command 'ollama' not found)")
                    } else {
                        trf("Could not start Ollama: {}", &[&e])
                    }
                })?;
            if let Ok(mut own) = OWN_SERVER.lock() {
                *own = Some(child);
            }
        }
    }
    for _ in 0..30 {
        if ollama::is_running() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err(tr("Ollama did not answer after 15 seconds"))
}

pub fn stop() -> Result<(), String> {
    if let Ok(mut own) = OWN_SERVER.lock()
        && let Some(mut child) = own.take()
    {
        let _ = child.kill();
        let _ = child.wait();
        return Ok(());
    }
    match systemd_service() {
        Some(true) => run_privileged(&["systemctl", "stop", "ollama.service"]),
        _ if ollama::is_running() => Err(tr(
            "Ollama was started outside this app (e.g. in a terminal). Please stop it there.",
        )),
        _ => Ok(()),
    }
}

pub fn is_own_running() -> bool {
    OWN_SERVER.lock().map(|own| own.is_some()).unwrap_or(false)
}

pub fn stop_own() {
    if let Ok(mut own) = OWN_SERVER.lock()
        && let Some(mut child) = own.take()
    {
        let _ = child.kill();
        let _ = child.wait();
    }
}

pub fn kill_command() -> String {
    if systemd_service().is_some() {
        "sudo pkill -f 'ollama runner'".to_string()
    } else {
        "pkill -f 'ollama runner'".to_string()
    }
}
