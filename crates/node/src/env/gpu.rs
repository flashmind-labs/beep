//! Graphics cards: `nvidia-smi` when it is installed, else `system_profiler`
//! on macOS or `lspci` on Linux.

use super::output;
use beep_proto::node::{GpuInfo, GpuVendor};
use serde_json::Value;
use std::env;

pub(crate) async fn gpus() -> Vec<GpuInfo> {
    if which::which("nvidia-smi").is_ok() {
        let args = [
            "--query-gpu=name,memory.total,driver_version",
            "--format=csv,noheader,nounits",
        ];
        let found = output("nvidia-smi", &args)
            .await
            .map(|text| parse_nvidia_smi(&text))
            .unwrap_or_default();
        if !found.is_empty() {
            return found;
        }
    }
    match env::consts::OS {
        "macos" => output("system_profiler", &["SPDisplaysDataType", "-json"])
            .await
            .map(|text| parse_displays_json(&text))
            .unwrap_or_default(),
        "linux" => output("lspci", &[])
            .await
            .map(|text| parse_lspci(&text))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The vendor a card's name or vendor string points to.
fn vendor_of(text: &str) -> GpuVendor {
    let text = text.to_lowercase();
    if text.contains("nvidia") {
        GpuVendor::Nvidia
    } else if text.contains("amd") || text.contains("ati") || text.contains("radeon") {
        GpuVendor::Amd
    } else if text.contains("intel") {
        GpuVendor::Intel
    } else if text.contains("apple") {
        GpuVendor::Apple
    } else {
        GpuVendor::Other
    }
}

/// `nvidia-smi --query-gpu=name,memory.total,driver_version
/// --format=csv,noheader,nounits`: one line per card.
fn parse_nvidia_smi(csv: &str) -> Vec<GpuInfo> {
    csv.lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, ',').map(str::trim);
            let model = fields.next().filter(|model| !model.is_empty())?;
            let vram_mb = fields.next()?.parse().ok()?;
            let driver = fields.next().filter(|driver| !driver.is_empty());
            Some(GpuInfo {
                vendor: GpuVendor::Nvidia,
                model: model.to_owned(),
                vram_mb: Some(vram_mb),
                driver: driver.map(str::to_owned),
            })
        })
        .collect()
}

/// `system_profiler SPDisplaysDataType -json`.
fn parse_displays_json(json: &str) -> Vec<GpuInfo> {
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(cards) = root.get("SPDisplaysDataType").and_then(Value::as_array) else {
        return Vec::new();
    };
    cards
        .iter()
        .filter_map(|card| {
            let model = card
                .get("sppci_model")
                .or_else(|| card.get("_name"))?
                .as_str()?;
            let vendor = card
                .get("spdisplays_vendor")
                .and_then(Value::as_str)
                .map_or_else(|| vendor_of(model), vendor_of);
            let vram_mb = card
                .get("spdisplays_vram")
                .and_then(Value::as_str)
                .and_then(vram_mb);
            Some(GpuInfo {
                vendor,
                model: model.to_owned(),
                vram_mb,
                driver: None,
            })
        })
        .collect()
}

/// Megabytes in a size such as "16 GB" or "8192 MB".
fn vram_mb(size: &str) -> Option<u64> {
    let mut parts = size.split_whitespace();
    let amount: u64 = parts.next()?.parse().ok()?;
    match parts.next()? {
        "GB" => amount.checked_mul(1024),
        "MB" => Some(amount),
        _ => None,
    }
}

/// The display controllers in `lspci` output.
fn parse_lspci(text: &str) -> Vec<GpuInfo> {
    const CLASSES: [&str; 3] = [
        "VGA compatible controller",
        "3D controller",
        "Display controller",
    ];
    text.lines()
        .filter_map(|line| {
            let (_, rest) = line.split_once(' ')?;
            let (class, name) = rest.split_once(": ")?;
            if !CLASSES.contains(&class) {
                return None;
            }
            let name = match name.rfind(" (rev ") {
                Some(at) => &name[..at],
                None => name,
            };
            Some(GpuInfo {
                vendor: vendor_of(name),
                model: name.trim().to_owned(),
                vram_mb: None,
                driver: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_proto::node::{GpuInfo, GpuVendor};

    #[test]
    fn nvidia_smi_lists_each_gpu() {
        assert_eq!(
            parse_nvidia_smi("NVIDIA GeForce RTX 4090, 24564, 560.35.03\n"),
            [GpuInfo {
                vendor: GpuVendor::Nvidia,
                model: "NVIDIA GeForce RTX 4090".into(),
                vram_mb: Some(24564),
                driver: Some("560.35.03".into()),
            }]
        );
    }

    #[test]
    fn nvidia_smi_garbage_is_empty() {
        assert!(parse_nvidia_smi("NVIDIA-SMI has failed").is_empty());
    }

    #[test]
    fn displays_json_reads_apple_gpu() {
        let json = r#"{"SPDisplaysDataType": [{"_name": "Apple M2 Ultra",
            "spdisplays_vendor": "sppci_vendor_Apple", "sppci_model": "Apple M2 Ultra",
            "sppci_cores": "76"}]}"#;
        let gpus = parse_displays_json(json);
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].vendor, GpuVendor::Apple);
        assert_eq!(gpus[0].model, "Apple M2 Ultra");
        assert!(parse_displays_json("not json").is_empty());
    }

    #[test]
    fn lspci_reads_vga_lines() {
        let text = "00:00.0 Host bridge: Example Corp Root Complex\n\
            01:00.0 VGA compatible controller: NVIDIA Corporation AD102 [GeForce RTX 4090] (rev a1)\n\
            02:00.0 3D controller: Advanced Micro Devices, Inc. [AMD/ATI] Example\n";
        let gpus = parse_lspci(text);
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].vendor, GpuVendor::Nvidia);
        assert_eq!(gpus[0].model, "NVIDIA Corporation AD102 [GeForce RTX 4090]");
        assert_eq!(gpus[1].vendor, GpuVendor::Amd);
    }
}
