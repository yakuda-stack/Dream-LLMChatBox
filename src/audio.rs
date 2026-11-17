use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

const TARGET_RATE: u32 = 16_000;
const MAX_SECONDS: usize = 600;

pub struct MicDevice {
    pub id: String,
    pub name: String,
}

pub struct Recorder {
    stop: mpsc::Sender<()>,
    shared: Arc<Shared>,
    sample_rate: u32,
}

#[derive(Clone)]
pub struct Live {
    shared: Arc<Shared>,
    sample_rate: u32,
}

impl Live {
    pub fn is_recording(&self) -> bool {
        self.shared.recording.load(Ordering::Relaxed)
    }

    pub fn snapshot_from(&self, from: usize) -> Vec<f32> {
        let start = (from as u64 * self.sample_rate as u64 / TARGET_RATE as u64) as usize;
        let raw: Vec<f32> = self
            .shared
            .samples
            .lock()
            .map(|samples| samples.get(start..).map(<[f32]>::to_vec).unwrap_or_default())
            .unwrap_or_default();
        resample(&raw, self.sample_rate, TARGET_RATE)
    }
}

struct Shared {
    samples: Mutex<Vec<f32>>,
    level: AtomicU32,
    recording: AtomicBool,
    gain: AtomicU32,
}

pub fn list_devices() -> Vec<MicDevice> {
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let id = device.id().ok()?.to_string();
            let name = device
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| id.clone());
            Some(MicDevice { id, name })
        })
        .collect()
}

impl Recorder {
    pub fn start(device_id: &str, gain: f32) -> Result<Recorder, String> {
        let shared = Arc::new(Shared {
            samples: Mutex::new(Vec::new()),
            level: AtomicU32::new(0),
            recording: AtomicBool::new(false),
            gain: AtomicU32::new(gain.to_bits()),
        });
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
        let thread_shared = shared.clone();
        let wanted = device_id.to_string();

        thread::spawn(move || {
            let stream = match open_stream(&wanted, thread_shared) {
                Ok((stream, rate)) => {
                    let _ = ready_tx.send(Ok(rate));
                    stream
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _ = stop_rx.recv();
            drop(stream);
        });

        let sample_rate = ready_rx
            .recv()
            .map_err(|_| "Microphone thread stopped".to_string())??;
        Ok(Recorder {
            stop: stop_tx,
            shared,
            sample_rate,
        })
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::Relaxed))
    }

    pub fn set_gain(&self, gain: f32) {
        self.shared.gain.store(gain.to_bits(), Ordering::Relaxed);
    }

    pub fn is_recording(&self) -> bool {
        self.shared.recording.load(Ordering::Relaxed)
    }

    pub fn live(&self) -> Live {
        Live {
            shared: self.shared.clone(),
            sample_rate: self.sample_rate,
        }
    }

    pub fn begin_recording(&self) {
        if let Ok(mut samples) = self.shared.samples.lock() {
            samples.clear();
        }
        self.shared.recording.store(true, Ordering::Relaxed);
    }

    pub fn finish_recording(&self) -> Vec<f32> {
        self.shared.recording.store(false, Ordering::Relaxed);
        let samples = self
            .shared
            .samples
            .lock()
            .map(|mut s| std::mem::take(&mut *s))
            .unwrap_or_default();
        resample(&samples, self.sample_rate, TARGET_RATE)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

fn open_stream(device_id: &str, shared: Arc<Shared>) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let device = if device_id.is_empty() {
        host.default_input_device()
    } else {
        device_id
            .parse()
            .ok()
            .and_then(|id| host.device_by_id(&id))
            .or_else(|| host.default_input_device())
    }
    .ok_or("No microphone found")?;

    let config = device
        .default_input_config()
        .map_err(|e| format!("Microphone config: {e}"))?;
    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&device, config.into(), channels, shared),
        cpal::SampleFormat::I16 => build::<i16>(&device, config.into(), channels, shared),
        cpal::SampleFormat::I32 => build::<i32>(&device, config.into(), channels, shared),
        cpal::SampleFormat::U16 => build::<u16>(&device, config.into(), channels, shared),
        cpal::SampleFormat::I8 => build::<i8>(&device, config.into(), channels, shared),
        other => return Err(format!("Unsupported microphone format: {other}")),
    }?;
    stream.play().map_err(|e| format!("Microphone start: {e}"))?;
    Ok((stream, sample_rate))
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let max_samples = config.sample_rate as usize * MAX_SECONDS;
    device
        .build_input_stream(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let gain = f32::from_bits(shared.gain.load(Ordering::Relaxed));
                let mono: Vec<f32> = data
                    .chunks(channels.max(1))
                    .map(|frame| {
                        let sum: f32 = frame.iter().map(|s| <f32 as FromSample<T>>::from_sample_(*s)).sum();
                        (sum / frame.len() as f32 * gain).clamp(-1.0, 1.0)
                    })
                    .collect();
                if mono.is_empty() {
                    return;
                }

                let rms: f32 = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len() as f32).sqrt();
                let db = 20.0 * rms.max(1e-6).log10();
                let level: f32 = ((db + 60.0) / 60.0).clamp(0.0, 1.0);
                let previous = f32::from_bits(shared.level.load(Ordering::Relaxed));
                let smoothed = if level > previous {
                    level
                } else {
                    previous * 0.85 + level * 0.15
                };
                shared.level.store(smoothed.to_bits(), Ordering::Relaxed);

                if shared.recording.load(Ordering::Relaxed)
                    && let Ok(mut samples) = shared.samples.lock()
                    && samples.len() < max_samples
                {
                    samples.extend_from_slice(&mono);
                }
            },
            |e| eprintln!("Microphone error: {e}"),
            None,
        )
        .map_err(|e| format!("Microphone stream: {e}"))
}

pub fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() {
        return samples.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let length = (samples.len() as f64 / ratio) as usize;
    (0..length)
        .map(|i| {
            let position = i as f64 * ratio;
            let index = position as usize;
            let fraction = (position - index as f64) as f32;
            let a = samples[index.min(samples.len() - 1)];
            let b = samples[(index + 1).min(samples.len() - 1)];
            a + (b - a) * fraction
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::resample;

    #[test]
    fn resample_length() {
        let input = vec![0.5f32; 48_000];
        let output = resample(&input, 48_000, 16_000);
        assert_eq!(output.len(), 16_000);
        assert!((output[100] - 0.5).abs() < 1e-6);
    }
}
