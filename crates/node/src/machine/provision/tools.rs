//! Installs `gh`, `teacli` and `flsh` in the node's own `bin` folder:
//! `Pinned::install` and `install_newest` for machines that lack them,
//! `install_release` for a release the server asks for.

use anyhow::{Context, Result, anyhow, bail, ensure};
use beep_proto::node::tool_update::{Releases, Tool};
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
        Asset {
            os: "windows",
            arch: "x86_64",
            url: "https://github.com/rayfish/teacli/releases/download/v1.0.3/teacli-windows-amd64.exe",
            sha256: "ab4e92027941d6df94aba163889d78be2defda65eba07767d5ab57ff79ccdc96",
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

    /// Downloads, checks against the pinned SHA-256 and installs the tool
    /// as `bin/<name>`.
    pub(super) async fn install(&self, bin: &Path) -> Result<PathBuf> {
        let asset = self.asset().with_context(|| {
            format!(
                "no {} release for {} {}",
                self.name,
                env::consts::OS,
                env::consts::ARCH
            )
        })?;
        let download = Download {
            name: self.name,
            url: asset.url.to_owned(),
            sha256: Sha256Of::Pinned(asset.sha256),
            member: asset.member.map(str::to_owned),
        };
        download.install(bin).await
    }
}

/// Installs the newest release of `tool`, as its channel's `latest.txt`
/// names it.
pub(super) async fn install_newest(tool: Tool, bin: &Path) -> Result<PathBuf> {
    let Releases::Channel(base) = tool.releases() else {
        bail!("{tool} has no release channel");
    };
    let version = curl(&format!("{base}/latest.txt"), None).await?;
    let version = String::from_utf8(version).context("reading the newest version")?;
    install_release(tool, version.trim(), bin).await
}

/// Installs release `version` of `tool` as `bin/<name>`, checked against
/// the checksums the release publishes.
pub(super) async fn install_release(tool: Tool, version: &str, bin: &Path) -> Result<PathBuf> {
    ensure!(
        !version.is_empty()
            && version
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-')),
        "{version} is not a version"
    );
    release(tool, version, env::consts::OS, env::consts::ARCH)?
        .install(bin)
        .await
}

/// Where release `version` of `tool` for `os` and `arch` is.
fn release(tool: Tool, version: &str, os: &str, arch: &str) -> Result<Download> {
    let base = match tool.releases() {
        Releases::GitHub(repo) => format!("https://github.com/{repo}/releases/download/v{version}"),
        Releases::Channel(base) => format!("{base}/{version}"),
    };
    let none = || anyhow!("no {tool} release for {os} {arch}");
    let go_arch = match arch {
        "x86_64" => Some("amd64"),
        "aarch64" => Some("arm64"),
        _ => None,
    };
    Ok(match tool {
        Tool::Gh => {
            let arch = go_arch.ok_or_else(none)?;
            let (os, ext) = match os {
                "linux" => ("linux", "tar.gz"),
                "macos" => ("macOS", "zip"),
                _ => return Err(none()),
            };
            let stem = format!("gh_{version}_{os}_{arch}");
            Download {
                name: "gh",
                url: format!("{base}/{stem}.{ext}"),
                sha256: Sha256Of::Listed(format!("{base}/gh_{version}_checksums.txt")),
                member: Some(format!("{stem}/bin/gh")),
            }
        }
        Tool::Teacli => {
            let arch = go_arch.ok_or_else(none)?;
            let (os, ext) = match os {
                "linux" => ("linux", ""),
                "macos" => ("darwin", ""),
                "windows" => ("windows", ".exe"),
                _ => return Err(none()),
            };
            Download {
                name: "teacli",
                url: format!("{base}/teacli-{os}-{arch}{ext}"),
                sha256: Sha256Of::Listed(format!("{base}/checksums.txt")),
                member: None,
            }
        }
        Tool::Flsh => {
            let target = flsh_target(os, arch).ok_or_else(none)?;
            let url = format!("{base}/flsh-{target}");
            Download {
                name: "flsh",
                sha256: Sha256Of::Alone(format!("{url}.sha256")),
                url,
                member: None,
            }
        }
    })
}

/// The target flsh is built for on `os` and `arch`; there is no Windows
/// build.
pub(super) fn flsh_target(os: &str, arch: &str) -> Option<String> {
    let os = match os {
        "linux" => "unknown-linux-gnu",
        "macos" => "apple-darwin",
        _ => return None,
    };
    matches!(arch, "x86_64" | "aarch64").then(|| format!("{arch}-{os}"))
}

/// One release file of a tool, to install.
struct Download {
    name: &'static str,
    url: String,
    sha256: Sha256Of,
    /// The program's path inside the archive; `None` when the file is the
    /// program itself.
    member: Option<String>,
}

/// Where the SHA-256 a download must have comes from.
enum Sha256Of {
    Pinned(&'static str),
    /// The release's checksums file at this URL, as `sha256sum` prints it.
    Listed(String),
    /// A file at this URL holding only the download's SHA-256.
    Alone(String),
}

impl Download {
    async fn install(&self, bin: &Path) -> Result<PathBuf> {
        tokio::fs::create_dir_all(bin)
            .await
            .with_context(|| format!("creating {}", bin.display()))?;
        let work = bin.join(format!(".download-{}", self.name));
        // A download left from a stopped node starts over.
        remove_dir(&work).await?;
        tokio::fs::create_dir_all(&work).await?;
        let installed = self.install_in(bin, &work).await;
        if let Err(e) = remove_dir(&work).await {
            log::warn!("could not clean up {}: {e:#}", work.display());
        }
        installed
    }

    async fn install_in(&self, bin: &Path, work: &Path) -> Result<PathBuf> {
        let file_name = self.url.rsplit('/').next().unwrap_or(self.name);
        let sha256 = match &self.sha256 {
            Sha256Of::Pinned(sha256) => (*sha256).to_owned(),
            Sha256Of::Listed(url) => {
                let sums = work.join("checksums.txt");
                curl(url, Some(&sums)).await?;
                let text = tokio::fs::read_to_string(&sums)
                    .await
                    .context("reading the release's checksums")?;
                listed_sha256(&text, file_name)
                    .with_context(|| format!("{file_name} is not in the release's checksums"))?
                    .to_owned()
            }
            Sha256Of::Alone(url) => {
                let sum = String::from_utf8(curl(url, None).await?)
                    .context("reading the release's checksum")?;
                sum.trim().to_owned()
            }
        };
        let download = work.join(file_name);
        curl(&self.url, Some(&download)).await?;
        let file = download.clone();
        tokio::task::spawn_blocking(move || verify(&file, &sha256)).await??;
        let program = match &self.member {
            Some(member) => {
                extract(&download, member, work).await?;
                work.join(member)
            }
            None => download,
        };
        let executable = program.clone();
        tokio::task::spawn_blocking(move || crate::files::fsat::set_mode(&executable, 0o755))
            .await??;
        let target = bin.join(format!("{}{}", self.name, env::consts::EXE_SUFFIX));
        tokio::fs::rename(&program, &target)
            .await
            .with_context(|| format!("moving {} into place", self.name))?;
        Ok(target)
    }
}

/// The SHA-256 that `sums`, as `sha256sum` prints them, lists for `file`.
fn listed_sha256<'a>(sums: &'a str, file: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let sha256 = words.next()?;
        let name = words.next()?.trim_start_matches('*');
        (name == file).then_some(sha256)
    })
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
    let found = hex::encode(hasher.finalize());
    ensure!(
        found.eq_ignore_ascii_case(expected),
        "{} has SHA-256 {found}, not the expected {expected}",
        path.display()
    );
    Ok(())
}

/// Downloads `url` with curl, which every supported system has: to `to`,
/// or else into what it returns.
async fn curl(url: &str, to: Option<&Path>) -> Result<Vec<u8>> {
    let mut command = Command::new("curl");
    command.args(["-fsSL", "--proto", "=https", "--retry", "2"]);
    if let Some(to) = to {
        command.arg("-o").arg(to);
    }
    let run = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
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
    Ok(out.stdout)
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

    #[test]
    fn the_checksum_refuses_another_file() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let file = dir.join("teacli");
        fs::write(&file, b"hello\n").unwrap();
        // `printf 'hello\n' | sha256sum`
        let right = "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";
        verify(&file, right).unwrap();
        verify(&file, &right.to_uppercase()).unwrap();
        fs::write(&file, b"hello, tampered\n").unwrap();
        let err = verify(&file, right).unwrap_err();
        assert!(format!("{err:#}").contains("not the expected"), "{err:#}");
    }

    #[test]
    fn a_release_is_checked_against_its_own_line() {
        let sums = "aaa  gh_2.103.3_linux_amd64.tar.gz\nbbb  gh_2.103.3_linux_arm64.tar.gz\n";
        let gh = release(Tool::Gh, "2.103.3", "linux", "aarch64").unwrap();
        assert_eq!(
            gh.url,
            "https://github.com/cli/cli/releases/download/v2.103.3/gh_2.103.3_linux_arm64.tar.gz"
        );
        assert_eq!(
            listed_sha256(sums, "gh_2.103.3_linux_arm64.tar.gz"),
            Some("bbb")
        );
        assert_eq!(listed_sha256(sums, "gh_2.103.3_macOS_arm64.zip"), None);
        let teacli = release(Tool::Teacli, "1.0.8", "macos", "x86_64").unwrap();
        assert!(
            teacli.url.ends_with("/v1.0.8/teacli-darwin-amd64"),
            "{}",
            teacli.url
        );
        let teacli = release(Tool::Teacli, "1.0.8", "windows", "x86_64").unwrap();
        assert!(
            teacli.url.ends_with("/v1.0.8/teacli-windows-amd64.exe"),
            "{}",
            teacli.url
        );
        let flsh = release(Tool::Flsh, "0.2.0", "macos", "aarch64").unwrap();
        assert_eq!(
            flsh.url,
            "https://releases.flashmind.com/flashmind/cli/0.2.0/flsh-aarch64-apple-darwin"
        );
        assert!(release(Tool::Flsh, "0.2.0", "windows", "x86_64").is_err());
    }
}
