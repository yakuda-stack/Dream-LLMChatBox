use crate::settings;
use crate::workspace;
use std::path::Path;

pub const DEFAULT_IMAGE: &str = "docker.io/library/debian:stable-slim";

pub fn engine_index(engine: &str) -> i32 {
    match engine {
        "podman" => 1,
        "docker" => 2,
        _ => 0,
    }
}

pub fn engine_name(index: i32) -> String {
    match index {
        1 => "podman".to_string(),
        2 => "docker".to_string(),
        _ => String::new(),
    }
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub fn image(current: &settings::Settings) -> String {
    let image = current.container_image.trim();
    if image.is_empty() { DEFAULT_IMAGE.to_string() } else { image.to_string() }
}

pub fn script(engine: &str, image: &str, directory: &Path, command: &str) -> String {
    let user = if engine == "podman" {
        "--userns=keep-id".to_string()
    } else {
        "--user \"$(id -u):$(id -g)\"".to_string()
    };
    format!(
        "{engine} run --rm --init {user} -v {}:/work:Z -w /work {} sh -c {}",
        quote(&directory.to_string_lossy()),
        quote(image),
        quote(command)
    )
}

pub fn wrap(command: &str, directory: &Path) -> Result<Option<String>, String> {
    let current = settings::load();
    let engine = current.container_engine.as_str();
    if engine.is_empty() || !workspace::is_inside(&directory.to_string_lossy()) {
        return Ok(None);
    }
    if !crate::documents::which(engine) {
        return Err(format!("{engine} is not installed – install it or switch the container off in Settings"));
    }
    Ok(Some(script(engine, &image(&current), directory, command)))
}

pub fn note() -> Option<String> {
    let current = settings::load();
    let engine = current.container_engine.as_str();
    (!engine.is_empty()).then(|| {
        format!(
            " Terminal commands run inside a {engine} container ({}); the test folder is mounted at /work. Install missing tools inside the command if needed.",
            image(&current)
        )
    })
}

pub fn status(engine: &str) -> String {
    if engine.is_empty() {
        crate::i18n::tr("Commands run directly on your computer.")
    } else if crate::documents::which(engine) {
        crate::i18n::trf("{} found ✓ – commands in test environments run in a container.", &[&engine])
    } else {
        crate::i18n::trf("{} is not installed.", &[&engine])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_command() {
        let text = script("podman", "debian:stable", Path::new("/tmp/work dir"), "echo 'hi'");
        assert_eq!(
            text,
            "podman run --rm --init --userns=keep-id -v '/tmp/work dir':/work:Z -w /work 'debian:stable' sh -c 'echo '\\''hi'\\'''"
        );
        assert!(script("docker", "x", Path::new("/a"), "ls").contains("--user \"$(id -u):$(id -g)\""));
        assert_eq!(engine_name(engine_index("docker")), "docker");
        assert_eq!(engine_name(0), "");
    }
}
