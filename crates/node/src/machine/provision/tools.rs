//! Installs `gh` and `teacli` in the node's own `bin` folder and `flsh` in
//! the user's `~/.local/bin`, where its own installer puts it:
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
use tokio::time::timeout;

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

/// teacli 1.0.13, the Forgejo and Gitea command line.
const TEACLI: Pinned = Pinned {
    name: "teacli",
    assets: &[
        Asset {
            os: "linux",
            arch: "x86_64",
            url: "https://github.com/rayfish/teacli/releases/download/v1.0.13/teacli-linux-amd64",
            sha256: "6592bcb433b4d58227c8c93edfe1594412809f9f2a170ced723c21d50ba19af3",
            member: None,
        },
        Asset {
            os: "linux",
            arch: "aarch64",
            url: "https://github.com/rayfish/teacli/releases/download/v1.0.13/teacli-linux-arm64",
            sha256: "715078187efdefa9f9edebce95572f5ad254a611acfa4904b6966089c7387077",
            member: None,
        },
        Asset {
            os: "macos",
            arch: "x86_64",
            url: "https://github.com/rayfish/teacli/releases/download/v1.0.13/teacli-darwin-amd64",
            sha256: "9f8d3a5a58e7f59bed83c354dd958e2fb7f816675dea94a946868c6eb6013097",
            member: None,
        },
        Asset {
            os: "macos",
            arch: "aarch64",
            url: "https://github.com/rayfish/teacli/releases/download/v1.0.13/teacli-darwin-arm64",
            sha256: "604d8f1ef3979bbdc92a4b361e108dd2f7a567505909f70ba551432d4463d2d0",
            member: None,
        },
        Asset {
            os: "windows",
            arch: "x86_64",
            url: "https://github.com/rayfish/teacli/releases/download/v1.0.13/teacli-windows-amd64.exe",
            sha256: "e252d66f8d5d946ce3934e7256077a6e1d188eb1f92df6fdbcfb8762192fcacd",
            member: None,
        },
    ],
};

pub(super) const PINNED: &[Pinned] = &[GH, TEACLI];

/// sccache 0.19.0, from <https://github.com/mozilla/sccache/releases>.
pub(super) const SCCACHE: Pinned = Pinned {
    name: "sccache",
    assets: &[
        Asset {
            os: "linux",
            arch: "x86_64",
            url: "https://github.com/mozilla/sccache/releases/download/v0.19.0/sccache-v0.19.0-x86_64-unknown-linux-musl.tar.gz",
            sha256: "aa73c7270c997894038c0d8f0d297865f57569e1687883322bc431713d4bb9dd",
            member: Some("sccache-v0.19.0-x86_64-unknown-linux-musl/sccache"),
        },
        Asset {
            os: "linux",
            arch: "aarch64",
            url: "https://github.com/mozilla/sccache/releases/download/v0.19.0/sccache-v0.19.0-aarch64-unknown-linux-musl.tar.gz",
            sha256: "3e47fc4a71b1ddd1b4294a0de9d34498922fdf41e3cd0bda5df3954e3d6deb9d",
            member: Some("sccache-v0.19.0-aarch64-unknown-linux-musl/sccache"),
        },
        Asset {
            os: "macos",
            arch: "x86_64",
            url: "https://github.com/mozilla/sccache/releases/download/v0.19.0/sccache-v0.19.0-x86_64-apple-darwin.tar.gz",
            sha256: "2db7344bb96de350df79cd9f0124dd4e9baa47b3de2fac1e63e1346b66b20684",
            member: Some("sccache-v0.19.0-x86_64-apple-darwin/sccache"),
        },
        Asset {
            os: "macos",
            arch: "aarch64",
            url: "https://github.com/mozilla/sccache/releases/download/v0.19.0/sccache-v0.19.0-aarch64-apple-darwin.tar.gz",
            sha256: "a35a0b7d622618774bc1bb8102d72562dfb21e619f88b153b370d7f89bc286af",
            member: Some("sccache-v0.19.0-aarch64-apple-darwin/sccache"),
        },
        Asset {
            os: "windows",
            arch: "x86_64",
            url: "https://github.com/mozilla/sccache/releases/download/v0.19.0/sccache-v0.19.0-x86_64-pc-windows-msvc.tar.gz",
            sha256: "130e4f8eb672e76590e9993c2ab890c4763b88238577bb11d8062158c7698353",
            member: Some("sccache-v0.19.0-x86_64-pc-windows-msvc/sccache.exe"),
        },
        Asset {
            os: "windows",
            arch: "aarch64",
            url: "https://github.com/mozilla/sccache/releases/download/v0.19.0/sccache-v0.19.0-aarch64-pc-windows-msvc.tar.gz",
            sha256: "a680e8e6058b033cd6ff56d56b2219ca13f1c1a45f50fb3b04f4ed80851f5c2b",
            member: Some("sccache-v0.19.0-aarch64-pc-windows-msvc/sccache.exe"),
        },
    ],
};

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
            let ext = if os == "windows" { ".exe" } else { "" };
            let url = format!("{base}/flsh-{target}{ext}");
            Download {
                name: "flsh",
                sha256: Sha256Of::Alone(format!("{url}.sha256")),
                url,
                member: None,
            }
        }
    })
}

/// The target flsh is built for on `os` and `arch`.
pub(super) fn flsh_target(os: &str, arch: &str) -> Option<String> {
    let os = match (os, arch) {
        ("linux", _) => "unknown-linux-gnu",
        ("macos", _) => "apple-darwin",
        ("windows", "x86_64") => "pc-windows-msvc",
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
                unpack(&download, work).await?;
                work.join(member)
            }
            None => download,
        };
        let executable = program.clone();
        tokio::task::spawn_blocking(move || crate::fs::fsat::set_mode(&executable, 0o755))
            .await??;
        let target = bin.join(format!("{}{}", self.name, env::consts::EXE_SUFFIX));
        tokio::fs::rename(&program, &target)
            .await
            .with_context(|| format!("moving {} into place", self.name))?;
        Ok(target)
    }
}

/// Where flsh is installed, as its own installer puts it: `~/.local/bin`.
pub(super) fn flsh_bin(home: &Path) -> PathBuf {
    home.join(".local").join("bin")
}

/// Moves a flsh the node installed in its own `node_bin` to `own_bin`, or
/// drops it when `own_bin` has one already, so only one flsh is found.
pub(super) fn settle_flsh(node_bin: &Path, own_bin: &Path) -> io::Result<()> {
    let name = format!("flsh{}", env::consts::EXE_SUFFIX);
    let (old, own) = (node_bin.join(&name), own_bin.join(&name));

    if !old.is_file() {
        return Ok(());
    }

    if own.is_file() {
        return fs::remove_file(&old);
    }

    fs::create_dir_all(own_bin)?;

    // Another file system takes a copy.
    if fs::rename(&old, &own).is_err() {
        fs::copy(&old, &own)?;
        fs::remove_file(&old)?;
    }

    Ok(())
}

/// The SHA-256 that `sums`, as `sha256sum` prints them, lists for `file`.
pub(crate) fn listed_sha256<'a>(sums: &'a str, file: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let sha256 = words.next()?;
        let name = words.next()?.trim_start_matches('*');
        (name == file).then_some(sha256)
    })
}

/// Fails unless the file at `path` has the SHA-256 `expected` (hex).
pub(crate) fn verify(path: &Path, expected: &str) -> Result<()> {
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
pub(crate) async fn curl(url: &str, to: Option<&Path>) -> Result<Vec<u8>> {
    let mut command = crate::process::command("curl");
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

/// Unpacks the whole archive at `archive`, a `.zip` or a gzipped tarball,
/// into `into`, in this process: a system `tar` can fail where this works,
/// as GNU tar does in WSL 1. Symlinks and executable bits are kept, and no
/// entry lands outside `into`.
pub(crate) async fn unpack(archive: &Path, into: &Path) -> Result<()> {
    let (packed, folder) = (archive.to_owned(), into.to_owned());
    tokio::task::spawn_blocking(move || unpack_now(&packed, &folder))
        .await?
        .with_context(|| format!("unpacking {}", archive.display()))
}

fn unpack_now(archive: &Path, into: &Path) -> Result<()> {
    let file = io::BufReader::new(fs::File::open(archive)?);

    if archive.extension().is_some_and(|ext| ext == "zip") {
        zip::ZipArchive::new(file)?.extract(into)?;
        return Ok(());
    }

    let mut tarball = tar::Archive::new(flate2::read::GzDecoder::new(file));
    // Times are not needed, and setting them is one more call that can fail.
    tarball.set_preserve_mtime(false);
    tarball.set_overwrite(true);
    tarball.unpack(into)?;
    Ok(())
}

pub(crate) async fn remove_dir(dir: &Path) -> Result<()> {
    match tokio::fs::remove_dir_all(dir).await {
        Err(e) if e.kind() != io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("removing {}", dir.display()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "../../tests/machine_provision_tools.rs"]
mod tests;
