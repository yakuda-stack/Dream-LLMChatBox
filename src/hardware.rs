use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

pub struct Hardware {
    pub gpu_name: String,
    pub vram_total: u64,
    pub ram_total: u64,
}

pub struct Fit {
    pub vram_percent: i32,
    pub ram_bytes: u64,
    pub needed_bytes: u64,
    pub level: i32,
    pub vram_fill: i32,
    pub ram_fill: i32,
    pub vram_label: String,
    pub ram_label: String,
}

fn fill_percent(part: u64, total: u64) -> i32 {
    if total == 0 {
        0
    } else {
        ((part as f64 / total as f64) * 100.0).round().clamp(0.0, 100.0) as i32
    }
}

pub fn get() -> &'static Hardware {
    static HARDWARE: OnceLock<Hardware> = OnceLock::new();
    HARDWARE.get_or_init(detect)
}

fn detect() -> Hardware {
    let (gpu_name, vram_total) = detect_nvidia()
        .or_else(detect_sysfs)
        .unwrap_or_else(|| (String::new(), 0));
    Hardware {
        gpu_name,
        vram_total,
        ram_total: detect_ram(),
    }
}

fn detect_nvidia() -> Option<(String, u64)> {
    let output = Command::new("nvidia-smi")
        .args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    let gpus: Vec<(String, u64)> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (name, mib) = line.rsplit_once(',')?;
            let mib: u64 = mib.trim().parse().ok()?;
            Some((name.trim().to_string(), mib * 1024 * 1024))
        })
        .collect();
    let first = gpus.first()?;
    let total: u64 = gpus.iter().map(|gpu| gpu.1).sum();
    let name = if gpus.len() > 1 {
        format!("{}× {}", gpus.len(), first.0)
    } else {
        first.0.clone()
    };
    Some((name, total))
}

const AMDGPU_IDS: [&str; 2] = ["/usr/share/libdrm/amdgpu.ids", "/usr/local/share/libdrm/amdgpu.ids"];

fn read_trimmed(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default().trim().to_string()
}

fn detect_sysfs() -> Option<(String, u64)> {
    let mut cards: Vec<(PathBuf, String, u64)> = fs::read_dir("/sys/class/drm")
        .ok()?
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            name.strip_prefix("card")
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
        })
        .filter_map(|entry| {
            let device = entry.path().join("device");
            let vendor = read_trimmed(&device.join("vendor"));
            let is_amd = vendor == "0x1002";
            if is_amd && !device.join("gpu_busy_percent").exists() {
                return None;
            }
            if !is_amd && vendor != "0x8086" && vendor != "0x10de" {
                return None;
            }
            let vram: u64 = read_trimmed(&device.join("mem_info_vram_total"))
                .parse()
                .unwrap_or(0);
            Some((device, vendor, vram))
        })
        .collect();
    cards.sort_by_key(|card| std::cmp::Reverse(card.2));
    let (device, vendor, vram) = cards.into_iter().next()?;
    let name = if vendor == "0x1002" {
        amd_name(&device)
            .or_else(mesa_amd_name)
            .or_else(|| pci_name(&device))
            .unwrap_or_else(|| "AMD GPU".to_string())
    } else {
        pci_name(&device).unwrap_or_else(|| "GPU".to_string())
    };
    Some((clean_name(&name), vram))
}

fn amd_name(device: &Path) -> Option<String> {
    let id = read_trimmed(&device.join("device"));
    let revision = read_trimmed(&device.join("revision"));
    AMDGPU_IDS
        .iter()
        .find_map(|path| fs::read_to_string(path).ok())
        .and_then(|table| lookup_amdgpu_ids(&table, &id, &revision))
}

pub fn lookup_amdgpu_ids(table: &str, device: &str, revision: &str) -> Option<String> {
    let normalize = |value: &str| value.trim().trim_start_matches("0x").trim_start_matches("0X").to_uppercase();
    let device = normalize(device);
    let revision = normalize(revision);
    if device.is_empty() {
        return None;
    }
    table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.splitn(3, ',');
            Some((fields.next()?, fields.next()?, fields.next()?))
        })
        .find(|(id, rev, _)| normalize(id) == device && normalize(rev) == revision)
        .map(|(_, _, name)| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

fn mesa_amd_name() -> Option<String> {
    let output = Command::new("glxinfo").arg("-B").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout).to_string();
    let line = text.lines().find_map(|line| line.trim().strip_prefix("Device:"))?;
    let name = match line.find(" (") {
        Some(index) => &line[..index],
        None => line,
    }
    .trim();
    let lower = name.to_lowercase();
    (lower.contains("amd") || lower.contains("radeon")).then(|| name.to_string())
}

pub fn clean_name(name: &str) -> String {
    name.replace("(R)", "")
        .replace("(TM)", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn pci_name(device: &Path) -> Option<String> {
    let slot = fs::canonicalize(device)
        .ok()?
        .file_name()?
        .to_string_lossy()
        .to_string();
    let output = Command::new("lspci").args(["-mm", "-s", &slot]).output().ok()?;
    let line = String::from_utf8_lossy(&output.stdout).to_string();
    let fields: Vec<&str> = line.split('"').collect();
    let vendor = fields.get(3)?.to_string();
    let device = fields.get(5)?.to_string();
    let model = match (device.find('['), device.rfind(']')) {
        (Some(start), Some(end)) if end > start => device[start + 1..end].to_string(),
        _ => device,
    };
    let brand = if vendor.contains("AMD") {
        "AMD"
    } else if vendor.contains("NVIDIA") {
        "NVIDIA"
    } else if vendor.contains("Intel") {
        "Intel"
    } else {
        ""
    };
    Some(format!("{brand} {model}").trim().to_string())
}

fn detect_ram() -> u64 {
    fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| {
            let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
            let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
            Some(kib * 1024)
        })
        .unwrap_or(0)
}

impl Hardware {
    pub fn summary(&self) -> String {
        let gpu = if self.vram_total == 0 && self.gpu_name.contains("NVIDIA") {
            format!("{}  ·  VRAM unknown (nvidia-smi missing)", self.gpu_name)
        } else if self.vram_total == 0 {
            "No GPU detected".to_string()
        } else {
            format!("{}  ·  VRAM {}", self.gpu_name, format_gb(self.vram_total))
        };
        format!("{gpu}  ·  RAM {}", format_gb(self.ram_total))
    }

    pub fn estimate(&self, model_size: u64) -> Fit {
        let needed = (model_size as f64 * 1.15 + 0.5 * GIB) as u64;
        let usable_vram = (self.vram_total as f64 * 0.9) as u64;
        let usable_ram = (self.ram_total as f64 * 0.8) as u64;
        let in_vram = needed.min(usable_vram);
        let ram_bytes = needed - in_vram;
        let vram_percent = (in_vram as f64 / needed as f64 * 100.0).round() as i32;

        let level = if ram_bytes > usable_ram {
            4
        } else if ram_bytes == 0 {
            0
        } else if vram_percent >= 75 {
            1
        } else if vram_percent >= 40 {
            2
        } else {
            3
        };

        let too_large = level == 4;
        let shown_vram = if too_large { usable_vram } else { in_vram };
        let shown_ram = if too_large { needed.saturating_sub(usable_vram) } else { ram_bytes };
        Fit {
            vram_percent,
            ram_bytes,
            needed_bytes: needed,
            level,
            vram_fill: fill_percent(shown_vram, self.vram_total),
            ram_fill: fill_percent(shown_ram, self.ram_total),
            vram_label: format!(
                "VRAM {} / {}",
                format_gb(shown_vram),
                format_gb(self.vram_total)
            ),
            ram_label: format!(
                "RAM {} / {}",
                format_gb(shown_ram),
                format_gb(self.ram_total)
            ),
        }
    }
}

impl Fit {
    pub fn fit_text(&self) -> String {
        if self.level == 4 {
            crate::i18n::trf("Does not fit (needs ~{})", &[&format_gb(self.needed_bytes)])
        } else if self.ram_bytes == 0 {
            "100% VRAM".to_string()
        } else if self.vram_percent == 0 {
            crate::i18n::trf("CPU only  ·  ~{} RAM", &[&format_gb(self.ram_bytes)])
        } else {
            format!(
                "~{}% VRAM  ·  ~{} RAM",
                self.vram_percent,
                format_gb(self.ram_bytes)
            )
        }
    }

    pub fn speed_text(&self) -> &'static str {
        match self.level {
            0 => "Fast",
            1 => "Medium",
            2 => "Slow",
            3 => "Very slow",
            _ => "Too large",
        }
    }
}

pub fn format_gb(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / GIB)
}

pub fn format_size(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", gb * 1000.0)
    }
}

pub fn size_from_params(params_billion: f64) -> u64 {
    (params_billion * 0.62 * GIB) as u64
}

pub fn parse_params(text: &str) -> Option<f64> {
    let lower = text.to_lowercase();
    let bytes = lower.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let starts_number = bytes[i].is_ascii_digit()
            && (i == 0 || !bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'e');
        if starts_number {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            let unit = bytes.get(i).copied();
            let next_ok = bytes
                .get(i + 1)
                .is_none_or(|c| !c.is_ascii_alphanumeric());
            if next_ok
                && let Ok(value) = lower[start..i].parse::<f64>()
            {
                match unit {
                    Some(b'b') => return Some(value),
                    Some(b'm') => return Some(value / 1000.0),
                    _ => {}
                }
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::parse_params;

    #[test]
    fn params() {
        assert_eq!(parse_params("27b"), Some(27.0));
        assert_eq!(parse_params("e4b"), Some(4.0));
        assert_eq!(parse_params("Qwen3.8-27B-GGUF"), Some(27.0));
        assert_eq!(parse_params("Qwen3.6-35B-A3B-GGUF"), Some(35.0));
        assert_eq!(parse_params("gemma-3-270m"), Some(0.27));
        assert_eq!(parse_params("Llama-3.2-1.5B-Instruct"), Some(1.5));
        assert_eq!(parse_params("qwen3.5"), None);
    }
}

#[cfg(test)]
mod gpu_tests {
    use super::*;

    #[test]
    fn exact_amd_names() {
        let table = "# List of AMDGPU IDs\n7550,\tC0,\tAMD Radeon RX 9070 XT\n7550,\tC3,\tAMD Radeon RX 9070\n744C,\tC8,\tAMD Radeon RX 7900 XTX\n";
        assert_eq!(lookup_amdgpu_ids(table, "0x7550", "0xc0").as_deref(), Some("AMD Radeon RX 9070 XT"));
        assert_eq!(lookup_amdgpu_ids(table, "0x7550", "0xc3").as_deref(), Some("AMD Radeon RX 9070"));
        assert_eq!(lookup_amdgpu_ids(table, "0x7550", "0xc9"), None);
        assert_eq!(lookup_amdgpu_ids(table, "", ""), None);
        assert_eq!(clean_name("AMD Radeon(TM)  RX 9070 XT"), "AMD Radeon RX 9070 XT");
    }
}
