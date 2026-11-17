use crate::i18n::{tr, trf};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::collections::VecDeque;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const VOICES_URL: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/main";

pub const DOWNLOADABLE: [(&str, &str); 9] = [
    ("de_DE-thorsten-medium", "Deutsch – Thorsten (medium)"),
    ("de_DE-thorsten-high", "Deutsch – Thorsten (high)"),
    ("de_DE-kerstin-low", "Deutsch – Kerstin (low)"),
    ("de_DE-eva_k-x_low", "Deutsch – Eva K (x-low)"),
    ("en_US-lessac-medium", "English US – Lessac (medium)"),
    ("en_US-amy-medium", "English US – Amy (medium)"),
    ("en_GB-alba-medium", "English GB – Alba (medium)"),
    ("fr_FR-siwis-medium", "Français – Siwis (medium)"),
    ("es_ES-davefx-medium", "Español – Davefx (medium)"),
];

pub const PIPER_MISSING: &str = "piper-tts missing";

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}

pub fn piper_installed() -> bool {
    on_path("piper-tts")
}

pub fn install_command() -> String {
    for helper in ["paru", "yay"] {
        if on_path(helper) {
            return format!("{helper} -S piper-tts --mflags --nocheck");
        }
    }
    "git clone https://aur.archlinux.org/piper-tts.git /tmp/piper-tts && cd /tmp/piper-tts && makepkg -si --nocheck".to_string()
}

pub struct Voice {
    pub id: String,
    pub label: String,
    pub installed: bool,
}

fn voices_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("dream-llmchatbox")
        .join("voices")
}

pub fn voice_path(id: &str) -> PathBuf {
    voices_dir().join(format!("{id}.onnx"))
}

fn installed_ids() -> Vec<String> {
    let mut ids: Vec<String> = fs::read_dir(voices_dir())
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let id = name.strip_suffix(".onnx")?.to_string();
                    voices_dir()
                        .join(format!("{id}.onnx.json"))
                        .exists()
                        .then_some(id)
                })
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

pub fn voices() -> Vec<Voice> {
    let installed = installed_ids();
    let mut list: Vec<Voice> = installed
        .iter()
        .map(|id| Voice {
            label: DOWNLOADABLE
                .iter()
                .find(|(known, _)| known == id)
                .map(|(_, label)| label.to_string())
                .unwrap_or_else(|| id.clone()),
            id: id.clone(),
            installed: true,
        })
        .collect();
    for (id, label) in DOWNLOADABLE {
        if !installed.iter().any(|i| i == id) {
            list.push(Voice {
                id: id.to_string(),
                label: label.to_string(),
                installed: false,
            });
        }
    }
    list
}

pub fn default_voice() -> String {
    let installed = installed_ids();
    if let Some(first) = installed.first() {
        return first.clone();
    }
    let german = std::env::var("LANG")
        .map(|lang| lang.starts_with("de"))
        .unwrap_or(false);
    if german {
        "de_DE-thorsten-medium".to_string()
    } else {
        "en_US-lessac-medium".to_string()
    }
}

fn voice_url(id: &str) -> Option<String> {
    let mut parts = id.splitn(3, '-');
    let language = parts.next()?;
    let name = parts.next()?;
    let quality = parts.next()?;
    let family = language.split('_').next()?;
    Some(format!("{VOICES_URL}/{family}/{language}/{name}/{quality}/{id}.onnx"))
}

pub fn download(id: &str, mut progress: impl FnMut(u32)) -> Result<(), String> {
    let url = voice_url(id).ok_or(tr("Unknown voice"))?;
    fs::create_dir_all(voices_dir()).map_err(|e| e.to_string())?;
    download_file(&format!("{url}.json"), &voices_dir().join(format!("{id}.onnx.json")), |_| {})?;
    download_file(&url, &voice_path(id), &mut progress)
}

fn download_file(url: &str, target: &Path, mut progress: impl FnMut(u32)) -> Result<(), String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .build()
        .into();
    let response = agent.get(url).call().map_err(|e| trf("Download failed: {}", &[&e]))?;
    let total: u64 = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let part = target.with_extension("part");
    let mut file = fs::File::create(&part).map_err(|e| e.to_string())?;
    let mut reader = response.into_body().into_reader();
    let mut buffer = vec![0u8; 1 << 16];
    let mut done: u64 = 0;
    loop {
        let read = reader.read(&mut buffer).map_err(|e| trf("Download failed: {}", &[&e]))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
        done += read as u64;
        if let Some(percent) = (done * 100).checked_div(total) {
            progress(percent as u32);
        }
    }
    file.flush().map_err(|e| e.to_string())?;
    fs::rename(&part, target).map_err(|e| e.to_string())
}

pub fn add_voice_file(onnx: &Path) -> Result<String, String> {
    let json = PathBuf::from(format!("{}.json", onnx.display()));
    if !json.exists() {
        return Err(tr("The matching .onnx.json file must be next to the .onnx file"));
    }
    let id = onnx
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .ok_or(tr("Invalid file name"))?;
    fs::create_dir_all(voices_dir()).map_err(|e| e.to_string())?;
    fs::copy(onnx, voice_path(&id)).map_err(|e| e.to_string())?;
    fs::copy(&json, voices_dir().join(format!("{id}.onnx.json"))).map_err(|e| e.to_string())?;
    Ok(id)
}

fn sample_rate(id: &str) -> u32 {
    fs::read_to_string(voices_dir().join(format!("{id}.onnx.json")))
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|value| value["audio"]["sample_rate"].as_u64())
        .unwrap_or(22050) as u32
}

pub struct Speaker {
    child: Arc<Mutex<Option<Child>>>,
    stop: Arc<AtomicBool>,
}

impl Speaker {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock()
            && let Some(child) = child.as_mut()
        {
            let _ = child.kill();
        }
    }
}

impl Drop for Speaker {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn speak(text: &str, voice: &str, on_done: impl FnOnce(Result<(), String>) + Send + 'static) -> Result<Speaker, String> {
    let model = voice_path(voice);
    if !model.exists() {
        return Err(trf("Voice {} is not installed", &[&voice]));
    }
    let rate = sample_rate(voice);

    let mut child = Command::new("piper-tts")
        .arg("--model")
        .arg(&model)
        .arg("--output-raw")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                PIPER_MISSING.to_string()
            } else {
                trf("Could not start piper-tts: {}", &[&e])
            }
        })?;

    let mut stdin = child.stdin.take().ok_or("piper-tts stdin")?;
    let mut stdout = child.stdout.take().ok_or("piper-tts stdout")?;
    let input: String = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
        let _ = stdin.write_all(b"\n");
    });

    let child = Arc::new(Mutex::new(Some(child)));
    let stop = Arc::new(AtomicBool::new(false));
    let queue: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));
    let finished = Arc::new(AtomicBool::new(false));

    let reader_queue = queue.clone();
    let reader_finished = finished.clone();
    let reader_stop = stop.clone();
    thread::spawn(move || {
        let mut buffer = vec![0u8; 8192];
        let mut leftover: Option<u8> = None;
        loop {
            if reader_stop.load(Ordering::SeqCst) {
                break;
            }
            let read = match stdout.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let mut bytes: Vec<u8> = leftover.take().into_iter().collect();
            bytes.extend_from_slice(&buffer[..read]);
            if bytes.len() % 2 == 1 {
                leftover = bytes.pop();
            }
            if let Ok(mut queue) = reader_queue.lock() {
                queue.extend(
                    bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| i16::from_le_bytes(*b) as f32 / 32768.0),
                );
            }
        }
        reader_finished.store(true, Ordering::SeqCst);
    });

    let player_child = child.clone();
    let player_stop = stop.clone();
    thread::spawn(move || {
        let result = play(queue, rate, finished, player_stop.clone());
        if let Ok(mut child) = player_child.lock()
            && let Some(mut child) = child.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
        on_done(result);
    });

    Ok(Speaker { child, stop })
}

fn play(
    queue: Arc<Mutex<VecDeque<f32>>>,
    source_rate: u32,
    finished: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> Result<(), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(tr("No audio output found"))?;
    let config = device
        .default_output_config()
        .map_err(|e| trf("Audio output: {}", &[&e]))?;
    let device_rate = config.sample_rate();
    let channels = config.channels() as usize;
    let step = source_rate as f64 / device_rate as f64;
    let drained = Arc::new(AtomicBool::new(false));

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, config.into(), channels, step, queue.clone(), drained.clone()),
        cpal::SampleFormat::I16 => build::<i16>(&device, config.into(), channels, step, queue.clone(), drained.clone()),
        cpal::SampleFormat::I32 => build::<i32>(&device, config.into(), channels, step, queue.clone(), drained.clone()),
        cpal::SampleFormat::U16 => build::<u16>(&device, config.into(), channels, step, queue.clone(), drained.clone()),
        other => return Err(trf("Unsupported audio format: {}", &[&other])),
    }?;
    stream.play().map_err(|e| trf("Audio output: {}", &[&e]))?;

    loop {
        thread::sleep(Duration::from_millis(50));
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let empty = queue.lock().map(|q| q.is_empty()).unwrap_or(true);
        if finished.load(Ordering::SeqCst) && empty && drained.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(150));
            break;
        }
    }
    drop(stream);
    Ok(())
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    step: f64,
    queue: Arc<Mutex<VecDeque<f32>>>,
    drained: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let mut position = 0.0f64;
    let mut current = 0.0f32;
    let mut next = 0.0f32;
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
                let Ok(mut queue) = queue.lock() else {
                    return;
                };
                let mut had_data = false;
                for frame in data.chunks_mut(channels.max(1)) {
                    while position >= 1.0 {
                        position -= 1.0;
                        current = next;
                        next = queue.pop_front().unwrap_or(0.0);
                    }
                    let sample = current + (next - current) * position as f32;
                    position += step;
                    if sample != 0.0 {
                        had_data = true;
                    }
                    for out in frame.iter_mut() {
                        *out = T::from_sample(sample);
                    }
                }
                drained.store(!had_data && queue.is_empty(), Ordering::SeqCst);
            },
            |e| eprintln!("Audio output error: {e}"),
            None,
        )
        .map_err(|e| trf("Audio output: {}", &[&e]))
}

#[cfg(test)]
mod tests {
    use super::voice_url;

    #[test]
    fn urls() {
        assert_eq!(
            voice_url("de_DE-thorsten-medium").unwrap(),
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/de/de_DE/thorsten/medium/de_DE-thorsten-medium.onnx"
        );
        assert_eq!(
            voice_url("de_DE-eva_k-x_low").unwrap(),
            "https://huggingface.co/rhasspy/piper-voices/resolve/main/de/de_DE/eva_k/x_low/de_DE-eva_k-x_low.onnx"
        );
    }
}
