//! The platforms a release has a build for, and how their archives are
//! named and packed.

use super::types::{Error, Version};
use std::env;

/// A platform a release has a build for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::AsRefStr, strum::Display)]
pub enum Target {
    #[strum(serialize = "x86_64-unknown-linux-musl")]
    LinuxX86_64,
    #[strum(serialize = "aarch64-unknown-linux-musl")]
    LinuxAarch64,
    #[strum(serialize = "aarch64-apple-darwin")]
    MacAarch64,
    #[strum(serialize = "x86_64-apple-darwin")]
    MacX86_64,
    /// The x64 build, which Windows on Arm runs too.
    #[strum(serialize = "x86_64-pc-windows-msvc")]
    WindowsX86_64,
}

/// How a release archive is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::AsRefStr)]
pub(super) enum Format {
    #[strum(serialize = "tar.gz")]
    TarGz,
    #[strum(serialize = "zip")]
    Zip,
}

impl From<Target> for Format {
    fn from(target: Target) -> Self {
        match target {
            Target::WindowsX86_64 => Self::Zip,
            Target::LinuxX86_64
            | Target::LinuxAarch64
            | Target::MacAarch64
            | Target::MacX86_64 => Self::TarGz,
        }
    }
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
            ("macos", "x86_64") => Ok(Self::MacX86_64),
            ("windows", "x86_64" | "aarch64") => Ok(Self::WindowsX86_64),
            _ => Err(Error::Platform { os, arch }),
        }
    }

    /// The release asset for `version`, named as the installers name it.
    pub fn archive(self, version: &Version) -> String {
        let format = Format::from(self);
        format!("beep-node-{version}-{self}.{}", format.as_ref())
    }

    /// The binary inside the archive.
    pub(super) fn binary(self) -> &'static str {
        match Format::from(self) {
            Format::TarGz => "beep-node",
            Format::Zip => "beep-node.exe",
        }
    }
}
