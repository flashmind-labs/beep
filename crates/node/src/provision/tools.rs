//! `gh` and `teacli` for machines that lack them: a pinned release for the
//! machine's OS and architecture, checked against its pinned SHA-256 and
//! put in the node's own `bin` folder.

use anyhow::{Context, Result, bail, ensure};
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{process::Command, time::timeout};

/// The longest one download may take.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// A tool the node installs when the machine lacks it.
pub(super) struct Pinned {
    pub(super) name: &'static str,
    assets: &'static [Asset],
}

/// One release file of a tool.
struct Asset {
    /// As `std::env::consts::OS` names it.
    os: &'static str,
    /// As `std::env::consts::ARCH` names it.
    arch: &'static str,
    url: &'static str,
    sha256: &'static str,
    /// The program's path inside the archive; `None` when the file is the
    /// program itself.
    member: Option<&'static str>,
}

/// gh 2.103.3, from <https://github.com/cli/cli/releases>.
const GH: Pinned = Pinned {
    name: "gh",
    assets: &[
        Asset {
            os: "linux",
            arch: "x86_64",
            url: "https://github.com/cli/cli/releases/download/v2.103.3/gh_2.103.3_linux_amd64.tar.gz",
            sha256: "4c806978ffce1cec70853302a56d70ac6da4f19bae7adca4e1a8ab00633dd91d",
            member: Some("gh_2.103.3_linux_amd64/bin/gh"),
        },
        Asset {
            os: "linux",
            arch: "aarch64",
            url: "https://github.com/cli/cli/releases/download/v2.103.3/gh_2.103.3_linux_arm64.tar.gz",
            sha256: "ee32df2323c8847f5fac6cb6b4b69ca75828e9329a8f22f677021f7b3306ce63",
            member: Some("gh_2.103.3_linux_arm64/bin/gh"),
        },
        Asset {
            os: "macos",
            arch: "x86_64",
            url: "https://github.com/cli/cli/releases/download/v2.103.3/gh_2.103.3_macOS_amd64.zip",
            sha256: "a2f86418f4350ae6e9ec60d5000bdf7665728436946a97fc27fe50e6ae752682",
            member: Some("gh_2.103.3_macOS_amd64/bin/gh"),
        },
        Asset {
            os: "macos",
            arch: "aarch64",
            url: "https://github.com/cli/cli/releases/download/v2.103.3/gh_2.103.3_macOS_arm64.zip",
            sha256: "d700207eae9ae98fe2b89535c2c087c07823d4628c56364695a5a07ef311c062",
            member: Some("gh_2.103.3_macOS_arm64/bin/gh"),
        },
    ],
};

/// teacli 1.0.3, the Forgejo and Gitea command line.
const TEACLI: Pinned = Pinned {
    name: "teacli",
    assets: &[
        Asset {
            os: "linux",
            arch: "x86_64",
            url: "https://github.com/rayfish/teacli/releases/download/v1.0.3/teacli-linux-amd64",
            sha256: "403550db95f4641a69a99fb41db35aa68ad7efa228749ac6e27b725452752bf3",
            member: None,
        },
        Asset {
            os: "linux",
            arch: "aarch64",
            url: "https://github.com/rayfish/teacli/releases/download/v1.0.3/teacli-linux-arm64",
            sha256: "e5f6636869246b6bb5ed3fb2c40b0c106f5bb94b20dc56a65a2090c35fcae32e",
            member: None,
        },
        Asset {
            os: "macos",
            arch: "x86_64",
            url: "https://github.com/rayfish/teacli/releases/download/v1.0.3/teacli-darwin-amd64",
            sha256: "a87d7e7cad013166b6eb70e212405d3ea0737ee79dd406e7175ccb606f042501",
            member: None,
        },
        Asset {
            os: "macos",
            arch: "aarch64",
            url: "https://github.com/rayfish/teacli/releases/download/v1.0.3/teacli-darwin-arm64",
            sha256: "e196e7ccb04b4bca1ad9d1b227bef081e5e45c05a2b36881ff59cdcd87a906de",
            member: None,
        },
    ],
};

pub(super) const PINNED: &[Pinned] = &[GH, TEACLI];

impl Pinned {
    fn asset(&self) -> Option<&'static Asset> {
        self.assets
            .iter()
            .find(|asset| asset.os == env::consts::OS && asset.arch == env::consts::ARCH)
    }

    /// Downloads, checks and installs the tool as `bin/<name>`.
    pub(super) async fn install(&self, bin: &Path) -> Result<PathBuf> {
        let asset = self.asset().with_context(|| {
            format!(
                "no {} release for {} {}",
                self.name,
                env::consts::OS,
                env::consts::ARCH
            )
        })?;
        tokio::fs::create_dir_all(bin)
            .await
            .with_context(|| format!("creating {}", bin.display()))?;
        let work = bin.join(format!(".download-{}", self.name));
        // A download left from a stopped node starts over.
        remove_dir(&work).await?;
        tokio::fs::create_dir_all(&work).await?;
        let installed = self.install_in(asset, bin, &work).await;
        if let Err(e) = remove_dir(&work).await {
            log::warn!("could not clean up {}: {e:#}", work.display());
        }
        installed
    }

    async fn install_in(&self, asset: &Asset, bin: &Path, work: &Path) -> Result<PathBuf> {
        let file_name = asset.url.rsplit('/').next().unwrap_or(self.name);
        let download = work.join(file_name);
        fetch(asset.url, &download).await?;
        let file = download.clone();
        let sha256 = asset.sha256;
        tokio::task::spawn_blocking(move || verify(&file, sha256)).await??;
        let program = match asset.member {
            Some(member) => {
                extract(&download, member, work).await?;
                work.join(member)
            }
            None => download,
        };
        let executable = program.clone();
        tokio::task::spawn_blocking(move || crate::fsat::set_mode(&executable, 0o755)).await??;
        let target = bin.join(self.name);
        tokio::fs::rename(&program, &target)
            .await
            .with_context(|| format!("moving {} into place", self.name))?;
        Ok(target)
    }
}

/// Fails unless the file at `path` has the SHA-256 `expected` (hex).
pub(super) fn verify(path: &Path, expected: &str) -> Result<()> {
    let mut file = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0; 64 * 1024];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    let found: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    ensure!(
        found.eq_ignore_ascii_case(expected),
        "{} has SHA-256 {found}, not the pinned {expected}",
        path.display()
    );
    Ok(())
}

/// Downloads `url` to `to` with curl, which every supported system has.
async fn fetch(url: &str, to: &Path) -> Result<()> {
    let run = Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--retry", "2", "-o"])
        .arg(to)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output();
    let out = timeout(DOWNLOAD_TIMEOUT, run)
        .await
        .context("the download timed out")?
        .context("starting curl")?;
    if !out.status.success() {
        bail!(
            "curl failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Unpacks `member` of the archive at `archive` into `into`.
async fn extract(archive: &Path, member: &str, into: &Path) -> Result<()> {
    let is_zip = archive.extension().is_some_and(|ext| ext == "zip");
    let mut command = if is_zip {
        let mut command = Command::new("unzip");
        command
            .arg("-q")
            .arg("-o")
            .arg(archive)
            .arg(member)
            .arg("-d")
            .arg(into);
        command
    } else {
        let mut command = Command::new("tar");
        command
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(into)
            .arg(member);
        command
    };
    let out = command
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .context("starting the unpacker")?;
    if !out.status.success() {
        bail!(
            "unpacking {} failed: {}",
            archive.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

async fn remove_dir(dir: &Path) -> Result<()> {
    match tokio::fs::remove_dir_all(dir).await {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("removing {}", dir.display()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("beep-node-tools-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_checksum_refuses_another_file() {
        let dir = temp_dir("verify");
        let file = dir.join("teacli");
        fs::write(&file, b"hello\n").unwrap();
        // `printf 'hello\n' | sha256sum`
        let right = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
        verify(&file, right).unwrap();
        verify(&file, &right.to_uppercase()).unwrap();
        fs::write(&file, b"hello, tampered\n").unwrap();
        let err = verify(&file, right).unwrap_err();
        assert!(format!("{err:#}").contains("not the pinned"), "{err:#}");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_pinned_tool_has_a_release_for_each_supported_machine() {
        for tool in PINNED {
            for os in ["linux", "macos"] {
                for arch in ["x86_64", "aarch64"] {
                    let asset = tool
                        .assets
                        .iter()
                        .find(|a| a.os == os && a.arch == arch)
                        .unwrap_or_else(|| panic!("{} on {os} {arch}", tool.name));
                    assert_eq!(asset.sha256.len(), 64);
                    assert!(asset.url.starts_with("https://"));
                }
            }
        }
    }
}
