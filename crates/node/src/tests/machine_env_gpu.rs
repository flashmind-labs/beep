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
fn lspci_intel_is_not_amd() {
    let gpus = parse_lspci("00:02.0 VGA compatible controller: Intel Corporation UHD Graphics 630");
    assert_eq!(gpus.len(), 1);
    assert_eq!(gpus[0].vendor, GpuVendor::Intel);
}
