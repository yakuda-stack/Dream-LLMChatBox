use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub struct Output {
    pub text: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stopped: bool,
}

impl Output {
    pub fn summary(&self, text: &str) -> String {
        let text = text.trim_end();
        if self.stopped {
            return format!("{text}\n[stopped by the user]");
        }
        if self.timed_out {
            return format!("{text}\n[stopped after the time limit]");
        }
        match self.exit_code {
            Some(0) if text.is_empty() => "(no output)".to_string(),
            Some(0) => text.to_string(),
            Some(code) => format!("{text}\n[exit code {code}]"),
            None => format!("{text}\n[exit code ?]"),
        }
    }
}

pub fn run(
    script: &str,
    directory: &Path,
    env: &[(String, String)],
    timeout: Duration,
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Output, String> {
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(script)
        .current_dir(directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start bash: {e}"))?;

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = stdout.as_mut() {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).to_string()
    });
    let err_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(pipe) = stderr.as_mut() {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).to_string()
    });

    let started = Instant::now();
    let mut timed_out = false;
    let mut stopped = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                let cancelled = cancel.as_ref().is_some_and(|flag| flag.load(Ordering::SeqCst));
                if cancelled || started.elapsed() > timeout {
                    timed_out = !cancelled;
                    stopped = cancelled;
                    kill_group(child.id());
                    let _ = child.kill();
                    break child.wait().ok();
                }
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break None,
        }
    };
    let out = out_reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();

    let mut text = out.trim_end().to_string();
    if !err.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(err.trim_end());
    }
    Ok(Output {
        text,
        exit_code: status.and_then(|status| status.code()),
        timed_out,
        stopped,
    })
}

fn kill_group(pid: u32) {
    let _ = Command::new("kill")
        .args(["-TERM", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_and_exit_code() {
        let dir = std::env::temp_dir();
        let ok = run("echo hi; echo err >&2", &dir, &[], Duration::from_secs(5), None).unwrap();
        assert_eq!(ok.summary(&ok.text), "hi\nerr");
        let fail = run("exit 4", &dir, &[], Duration::from_secs(5), None).unwrap();
        assert_eq!(fail.summary(&fail.text), "\n[exit code 4]");
        let env = vec![("NAME".to_string(), "x; rm -rf y".to_string())];
        let var = run("printf '%s' \"$NAME\"", &dir, &env, Duration::from_secs(5), None).unwrap();
        assert_eq!(var.text, "x; rm -rf y");
        let slow = run("sleep 5", &dir, &[], Duration::from_millis(300), None).unwrap();
        assert!(slow.timed_out);
    }
}
