//! The platforms a release has a build for, and how their archives are
//! named and packed.

use super::{Error, version::Version};
use std::env;

/// A platform a release has a build for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    LinuxX86_64,
    LinuxAarch64,
    MacAarch64,
    /// The x64 build, which Windows on Arm runs too.
    WindowsX86_64,
}

/// How a release archive is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Format {
    TarGz,
    Zip,
}

impl Target {
    /// The build for this binary's platform.
    pub fn current() -> Result<Self, Error> {
        Self::detect(env::consts::OS, env::consts::ARCH)
    }

    /// The build for `os` and `arch`, as `std::env::consts` names them.
    pub fn detect(os: &'static str, arch: &'static str) -> Result<Self, Error> {
        match (os, arch) {
            ("linux", "x86_64") => Ok(Self::LinuxX86_64),
            ("linux", "aarch64") => Ok(Self::LinuxAarch64),
            ("macos", "aarch64") => Ok(Self::MacAarch64),
            ("macos", "x86_64") => Err(Error::IntelMac),
            ("windows", "x86_64" | "aarch64") => Ok(Self::WindowsX86_64),
            _ => Err(Error::Platform { os, arch }),
        }
    }

    /// The Rust target the build was made for, as the archive names it.
    pub fn triple(self) -> &'static str {
        match self {
            Self::LinuxX86_64 => "x86_64-unknown-linux-musl",
            Self::LinuxAarch64 => "aarch64-unknown-linux-musl",
            Self::MacAarch64 => "aarch64-apple-darwin",
            Self::WindowsX86_64 => "x86_64-pc-windows-msvc",
        }
    }

    pub(super) fn format(self) -> Format {
        match self {
            Self::WindowsX86_64 => Format::Zip,
            Self::LinuxX86_64 | Self::LinuxAarch64 | Self::MacAarch64 => Format::TarGz,
        }
    }

    /// The release asset for `version`, named as the installers name it.
    pub fn archive(self, version: &Version) -> String {
        let extension = match self.format() {
            Format::TarGz => "tar.gz",
            Format::Zip => "zip",
        };
        format!("beep-node-{version}-{}.{extension}", self.triple())
    }

    /// The binary inside the archive.
    pub(super) fn binary(self) -> &'static str {
        match self.format() {
            Format::TarGz => "beep-node",
            Format::Zip => "beep-node.exe",
        }
    }
}
