//! `beep-node update`: replaces this binary with the newest release and
//! restarts the service running it. It does what rerunning `install.sh` or
//! `install.ps1` does on an upgrade, for the binary that is running wherever
//! it lives: the same archives, checked against the release's `SHA256SUMS`.

use crate::service::{self, Restart};
use reqwest::{Client, StatusCode, Url};
use rustls::{ClientConfig, RootCertStore};
use sha2::{Digest, Sha256};
use std::{
    cmp::Ordering,
    env, fmt,
    fs::{self, File, OpenOptions},
    io::{self, Cursor, ErrorKind, Read, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use tokio::task::{JoinError, spawn_blocking};

/// Where releases are published; `BEEP_NODE_BASE` replaces it, as for the
/// installers.
const BASE: &str = "https://releases.flashmind.com/beep/node";

/// This binary's version.
const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Why the update stopped.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("bad version {0:?}")]
    BadVersion(String),
    #[error("bad download location {text:?}")]
    BadBase {
        text: String,
        #[source]
        source: <Url as FromStr>::Err,
    },
    #[error("could not set up HTTPS")]
    Tls(#[source] rustls::Error),
    #[error("could not set up downloads")]
    Client(#[source] reqwest::Error),
    #[error("could not reach {url}; is this machine online?")]
    Network {
        url: Url,
        #[source]
        source: reqwest::Error,
    },
    #[error("{url} answered {status}")]
    Status { url: Url, status: StatusCode },
    #[error("there is no beep-node build for {os} on {arch}")]
    Platform {
        os: &'static str,
        arch: &'static str,
    },
    #[error("beep-node runs on Macs with Apple silicon; there is no build for Intel Macs")]
    IntelMac,
    #[error("{archive} is not listed in SHA256SUMS")]
    NotListed { archive: String },
    #[error(
        "checksum mismatch for {archive} (expected {expected}, got {actual}); nothing was installed"
    )]
    Checksum {
        archive: String,
        expected: String,
        actual: String,
    },
    #[error("could not unpack {archive}")]
    Unpack {
        archive: String,
        #[source]
        source: io::Error,
    },
    #[error("{archive} holds no {binary}")]
    Missing {
        archive: String,
        binary: &'static str,
    },
    #[error(
        "cannot write to {}; rerun `beep-node update` as the user that owns it, \
         or with sudo if it is a system folder",
        dir.display()
    )]
    NoPermission { dir: PathBuf },
    #[error("{action} {}", path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("installed beep-node {version}, but could not restart its service")]
    Restart {
        version: Version,
        #[source]
        source: anyhow::Error,
    },
    #[error("the update stopped")]
    Join(#[from] JoinError),
}

/// A release's version: dot-separated numbers, compared as numbers, so
/// `0.1.10` is newer than `0.1.9` and `1.2` is `1.2.0`.
#[derive(Debug, Clone)]
pub struct Version {
    text: String,
    /// The numbers without trailing zeros.
    parts: Vec<u64>,
}

impl FromStr for Version {
    type Err = Error;

    fn from_str(text: &str) -> Result<Self, Error> {
        let bad = || Error::BadVersion(text.to_owned());
        let trimmed = text.trim();
        let trimmed = trimmed.strip_prefix('v').unwrap_or(trimmed);
        let mut parts = trimmed
            .split('.')
            .map(|part| {
                if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(bad());
                }
                part.parse::<u64>().map_err(|_| bad())
            })
            .collect::<Result<Vec<_>, _>>()?;
        while parts.last() == Some(&0) {
            parts.pop();
        }
        Ok(Self {
            text: trimmed.to_owned(),
            parts,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts.cmp(&other.parts)
    }
}

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
enum Format {
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

    fn format(self) -> Format {
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
    fn binary(self) -> &'static str {
        match self.format() {
            Format::TarGz => "beep-node",
            Format::Zip => "beep-node.exe",
        }
    }
}

/// `beep-node update`: installs the newest release over this binary and
/// restarts its service. With `check`, only says whether there is one.
pub async fn update(check: bool) -> Result<(), Error> {
    let base = base();
    let current: Version = CURRENT.parse()?;
    let client = client()?;
    let latest = fetch(&client, &url(&base, "latest.txt")?).await?;
    let latest: Version = String::from_utf8_lossy(&latest).parse()?;
    match current.cmp(&latest) {
        Ordering::Equal => {
            println!("beep-node {current} is the newest");
            return Ok(());
        }
        Ordering::Greater => {
            println!("beep-node {current} is newer than the latest release, {latest}");
            return Ok(());
        }
        Ordering::Less if check => {
            println!("beep-node {latest} is out; this is {current}. Run `beep-node update`.");
            return Ok(());
        }
        Ordering::Less => {}
    }

    let target = Target::current()?;
    let exe = env::current_exe()
        .and_then(|exe| exe.canonicalize())
        .map_err(|source| Error::Io {
            action: "finding",
            path: PathBuf::from("this binary"),
            source,
        })?;
    // Fails before the download when the binary cannot be replaced.
    let probe = exe.clone();
    spawn_blocking(move || writable(&probe)).await??;

    let archive = target.archive(&latest);
    println!("Downloading beep-node {latest} for {}", target.triple());
    let packed = fetch(&client, &url(&base, &format!("{latest}/{archive}"))?).await?;
    let sums = fetch(&client, &url(&base, &format!("{latest}/SHA256SUMS"))?).await?;
    let installed = exe.clone();
    spawn_blocking(move || {
        verify(&archive, &packed, &String::from_utf8_lossy(&sums))?;
        let binary = unpack(target, &archive, &packed)?;
        replace(&installed, &binary)
    })
    .await??;
    println!("Installed {}", exe.display());

    let restarted = {
        let exe = exe.clone();
        spawn_blocking(move || service::restart(&exe)).await?
    };
    let restarted = restarted.map_err(|source| Error::Restart {
        version: latest.clone(),
        source,
    })?;
    match restarted {
        Restart::Restarted(_) => println!("Restarted the beep-node service on {latest}."),
        Restart::Stopped(manager) => println!(
            "The beep-node service is stopped, so it was left so. Start it on {latest} with:\n  {}",
            manager.start_command()
        ),
        Restart::OtherBinary(file) => println!(
            "The beep-node service ({}) runs another binary, so it was not restarted.",
            file.display()
        ),
        Restart::NotInstalled => println!(
            "beep-node is not installed as a service here, so nothing was restarted. \
             A beep-node still running keeps the old version until it restarts."
        ),
    }
    Ok(())
}

/// The release folder, without a trailing slash.
fn base() -> String {
    env::var("BEEP_NODE_BASE")
        .ok()
        .filter(|base| !base.trim().is_empty())
        .map_or_else(
            || BASE.to_owned(),
            |base| base.trim_end_matches('/').to_owned(),
        )
}

fn url(base: &str, path: &str) -> Result<Url, Error> {
    let text = format!("{base}/{path}");
    Url::parse(&text).map_err(|source| Error::BadBase { text, source })
}

/// An HTTPS client with the same TLS setup iroh uses: ring, webpki roots.
fn client() -> Result<Client, Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let tls = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(Error::Tls)?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Client::builder()
        .use_preconfigured_tls(tls)
        .user_agent(concat!("beep-node/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(Error::Client)
}

async fn fetch(client: &Client, url: &Url) -> Result<Vec<u8>, Error> {
    let network = |source| Error::Network {
        url: url.clone(),
        source,
    };
    let response = client.get(url.clone()).send().await.map_err(network)?;
    let status = response.status();
    if !status.is_success() {
        return Err(Error::Status {
            url: url.clone(),
            status,
        });
    }
    Ok(response.bytes().await.map_err(network)?.to_vec())
}

/// Maps an I/O error on `path`, telling a missing permission apart.
fn io_error(action: &'static str, path: &Path) -> impl FnOnce(io::Error) -> Error {
    move |source| {
        if source.kind() == ErrorKind::PermissionDenied {
            let dir = path.parent().unwrap_or(path).to_owned();
            return Error::NoPermission { dir };
        }
        Error::Io {
            action,
            path: path.to_owned(),
            source,
        }
    }
}

/// The file the new binary is written to before it replaces `exe`.
fn staged(exe: &Path) -> PathBuf {
    let name = exe
        .file_name()
        .map_or_else(|| "beep-node".into(), |name| name.to_string_lossy());
    exe.with_file_name(format!(".{name}.new"))
}

/// Whether a file can be written next to `exe`.
fn writable(exe: &Path) -> Result<(), Error> {
    let path = staged(exe);
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(io_error("writing", &path))?;
    fs::remove_file(&path).map_err(io_error("removing", &path))
}

/// Checks `packed` against its line in `sums`, a `SHA256SUMS` file.
fn verify(archive: &str, packed: &[u8], sums: &str) -> Result<(), Error> {
    let expected = expected_sum(archive, sums).ok_or_else(|| Error::NotListed {
        archive: archive.to_owned(),
    })?;
    let actual = format!("{:x}", Sha256::digest(packed));
    if actual != expected {
        return Err(Error::Checksum {
            archive: archive.to_owned(),
            expected,
            actual,
        });
    }
    Ok(())
}

/// The checksum `sums` lists for `archive`, in either `sha256sum` mode.
fn expected_sum(archive: &str, sums: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        let sum = fields.next()?;
        let name = fields.next()?;
        let named = name == archive || name.strip_prefix('*') == Some(archive);
        (named && fields.next().is_none()).then(|| sum.to_ascii_lowercase())
    })
}

/// The binary inside the release archive.
fn unpack(target: Target, archive: &str, packed: &[u8]) -> Result<Vec<u8>, Error> {
    let unpack_error = |source| Error::Unpack {
        archive: archive.to_owned(),
        source,
    };
    let found = match target.format() {
        Format::TarGz => untar(packed, target.binary()),
        Format::Zip => unzip(packed, target.binary()),
    }
    .map_err(unpack_error)?;
    found.ok_or_else(|| Error::Missing {
        archive: archive.to_owned(),
        binary: target.binary(),
    })
}

/// The file named `binary` in a gzipped tarball.
fn untar(packed: &[u8], binary: &str) -> io::Result<Option<Vec<u8>>> {
    let mut tarball = tar::Archive::new(flate2::read::GzDecoder::new(packed));
    for entry in tarball.entries()? {
        let mut entry = entry?;
        let named = entry.path()?.file_name().is_some_and(|name| name == binary);
        if named && entry.header().entry_type().is_file() {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

/// The file named `binary` in a zip archive.
fn unzip(packed: &[u8], binary: &str) -> io::Result<Option<Vec<u8>>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(packed))?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        let named = file
            .enclosed_name()
            .is_some_and(|path| path.file_name().is_some_and(|name| name == binary));
        if named && file.is_file() {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

/// Puts `binary` in place of `exe` in one rename, so nothing ever runs a
/// half-written file. It keeps `exe`'s permissions.
fn replace(exe: &Path, binary: &[u8]) -> Result<(), Error> {
    let fresh = staged(exe);
    let written = write_fresh(exe, &fresh, binary);
    let swapped = written.and_then(|()| swap(exe, &fresh));
    if swapped.is_err() {
        // Best effort: what matters is the error that stopped the update.
        let _ = fs::remove_file(&fresh);
    }
    swapped
}

fn write_fresh(exe: &Path, fresh: &Path, binary: &[u8]) -> Result<(), Error> {
    let permissions = fs::metadata(exe)
        .map_err(io_error("reading", exe))?
        .permissions();
    let mut file = File::create(fresh).map_err(io_error("writing", fresh))?;
    file.write_all(binary)
        .and_then(|()| file.sync_all())
        .map_err(io_error("writing", fresh))?;
    drop(file);
    fs::set_permissions(fresh, permissions).map_err(io_error("setting permissions on", fresh))
}

/// Renames `fresh` over `exe`.
#[cfg(not(windows))]
fn swap(exe: &Path, fresh: &Path) -> Result<(), Error> {
    fs::rename(fresh, exe).map_err(io_error("replacing", exe))
}

/// Renames `fresh` over `exe`. A running `.exe` cannot be overwritten but
/// can be renamed, so the old one moves aside first.
#[cfg(windows)]
fn swap(exe: &Path, fresh: &Path) -> Result<(), Error> {
    swap_aside(exe, fresh, &aside(exe))
}

/// Where the old binary waits: `beep-node.exe.old`, the name `install.ps1`
/// uses too. It stays until the next update removes it.
#[cfg(any(windows, test))]
fn aside(exe: &Path) -> PathBuf {
    let mut name = exe.as_os_str().to_owned();
    name.push(".old");
    PathBuf::from(name)
}

/// Moves `exe` to `old`, then `fresh` to `exe`, putting `exe` back when the
/// second rename fails. Plain renames, so it runs anywhere; only Windows
/// needs it.
#[cfg(any(windows, test))]
fn swap_aside(exe: &Path, fresh: &Path, old: &Path) -> Result<(), Error> {
    match fs::remove_file(old) {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                action: "removing the copy an earlier update left (is a beep-node still running from it?)",
                path: old.to_owned(),
                source,
            });
        }
    }
    fs::rename(exe, old).map_err(io_error("moving aside", exe))?;
    if let Err(e) = fs::rename(fresh, exe) {
        // Best effort: puts the old binary back, so the machine keeps a
        // beep-node; the error that matters is the failed rename.
        let _ = fs::rename(old, exe);
        return Err(io_error("replacing", exe)(e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        text.parse().expect("a valid version")
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert!(version("0.1.10") > version("0.1.9"));
        assert!(version("0.2.0") > version("0.1.14"));
        assert!(version("1.0.0") > version("0.99.99"));
        assert!(version("0.1.14") < version("0.1.15"));
        assert_eq!(version("0.1.14"), version("0.1.14"));
        assert_eq!(version("1.2"), version("1.2.0"));
        assert_eq!(version("v0.1.14\n"), version("0.1.14"));
        assert_eq!(version(" 0.1.14 ").to_string(), "0.1.14");
    }

    #[test]
    fn junk_is_not_a_version() {
        for junk in ["", "1..2", "1.2.x", "1.2.3-beta", "<html>", "1.-2"] {
            assert!(junk.parse::<Version>().is_err(), "{junk:?}");
        }
    }

    #[test]
    fn this_binary_has_a_version() {
        assert!(CURRENT.parse::<Version>().is_ok());
    }

    #[test]
    fn each_platform_gets_the_installers_asset() {
        let v = version("0.1.15");
        let asset = |os, arch| Target::detect(os, arch).map(|target| target.archive(&v));
        assert_eq!(
            asset("linux", "x86_64").ok().as_deref(),
            Some("beep-node-0.1.15-x86_64-unknown-linux-musl.tar.gz")
        );
        assert_eq!(
            asset("linux", "aarch64").ok().as_deref(),
            Some("beep-node-0.1.15-aarch64-unknown-linux-musl.tar.gz")
        );
        assert_eq!(
            asset("macos", "aarch64").ok().as_deref(),
            Some("beep-node-0.1.15-aarch64-apple-darwin.tar.gz")
        );
        assert_eq!(
            asset("windows", "x86_64").ok().as_deref(),
            Some("beep-node-0.1.15-x86_64-pc-windows-msvc.zip")
        );
        assert_eq!(
            asset("windows", "aarch64").ok().as_deref(),
            Some("beep-node-0.1.15-x86_64-pc-windows-msvc.zip")
        );
    }

    #[test]
    fn platforms_without_a_build_are_named() {
        assert!(matches!(
            Target::detect("macos", "x86_64"),
            Err(Error::IntelMac)
        ));
        let other = Target::detect("freebsd", "x86_64");
        assert!(matches!(
            other,
            Err(Error::Platform {
                os: "freebsd",
                arch: "x86_64"
            })
        ));
        assert!(Target::detect("linux", "riscv64").is_err());
    }

    #[test]
    fn the_sum_is_read_in_either_mode() {
        let sums = "AAAA  beep-node-1-x.tar.gz\nbbbb *beep-node-1-y.zip\n";
        assert_eq!(
            expected_sum("beep-node-1-x.tar.gz", sums).as_deref(),
            Some("aaaa")
        );
        assert_eq!(
            expected_sum("beep-node-1-y.zip", sums).as_deref(),
            Some("bbbb")
        );
        assert_eq!(expected_sum("beep-node-1-z.zip", sums), None);
    }

    #[test]
    fn a_download_that_does_not_match_its_sum_is_refused() {
        let packed = b"archive";
        let sum = format!("{:x}", Sha256::digest(packed));
        let sums = format!("{sum}  a.tar.gz\n");
        assert!(verify("a.tar.gz", packed, &sums).is_ok());
        assert!(matches!(
            verify("a.tar.gz", b"tampered", &sums),
            Err(Error::Checksum { .. })
        ));
        assert!(matches!(
            verify("b.tar.gz", packed, &sums),
            Err(Error::NotListed { .. })
        ));
    }

    fn tarball(name: &str, body: &[u8]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, name, body)
            .expect("appending to an in-memory tarball");
        builder
            .into_inner()
            .and_then(|gz| gz.finish())
            .expect("finishing an in-memory tarball")
    }

    #[test]
    fn the_binary_comes_out_of_the_tarball() {
        let packed = tarball("beep-node", b"new binary");
        let found = unpack(Target::LinuxX86_64, "a.tar.gz", &packed).expect("unpacked");
        assert_eq!(found, b"new binary");

        let other = tarball("README", b"text");
        assert!(matches!(
            unpack(Target::MacAarch64, "a.tar.gz", &other),
            Err(Error::Missing { .. })
        ));
        assert!(matches!(
            unpack(Target::LinuxX86_64, "a.tar.gz", b"not gzip"),
            Err(Error::Unpack { .. })
        ));
    }

    #[test]
    fn the_binary_comes_out_of_the_zip() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("beep-node.exe", zip::write::SimpleFileOptions::default())
            .expect("starting an in-memory zip entry");
        zip.write_all(b"new exe").expect("writing to memory");
        let packed = zip
            .finish()
            .expect("finishing an in-memory zip")
            .into_inner();
        let found = unpack(Target::WindowsX86_64, "a.zip", &packed).expect("unpacked");
        assert_eq!(found, b"new exe");
    }

    #[test]
    fn the_binary_is_replaced_in_place() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let exe = dir.path().join(if cfg!(windows) {
            "beep-node.exe"
        } else {
            "beep-node"
        });
        fs::write(&exe, b"old").expect("writing the old binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&exe, fs::Permissions::from_mode(0o750)).expect("chmod");
        }
        writable(&exe).expect("the folder is writable");
        replace(&exe, b"new").expect("replaced");
        assert_eq!(fs::read(&exe).expect("reading it back"), b"new");
        assert!(!staged(&exe).exists(), "the staged copy was renamed");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&exe).expect("metadata").permissions().mode();
            assert_eq!(mode & 0o777, 0o750);
        }
    }

    #[test]
    fn the_old_binary_moves_aside_next_to_the_new_one() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let exe = dir.path().join("beep-node.exe");
        let fresh = staged(&exe);
        let old = aside(&exe);
        assert_eq!(old, dir.path().join("beep-node.exe.old"));
        fs::write(&exe, b"running").expect("writing the old binary");
        fs::write(&fresh, b"new").expect("writing the new binary");
        swap_aside(&exe, &fresh, &old).expect("swapped");
        assert_eq!(fs::read(&exe).expect("reading exe"), b"new");
        assert_eq!(fs::read(&old).expect("reading old"), b"running");
        assert!(!fresh.exists(), "the staged copy was renamed");
    }

    #[test]
    fn the_copy_an_earlier_update_left_is_replaced() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let exe = dir.path().join("beep-node.exe");
        let fresh = staged(&exe);
        let old = aside(&exe);
        fs::write(&old, b"two updates ago").expect("writing the leftover");
        fs::write(&exe, b"running").expect("writing the old binary");
        fs::write(&fresh, b"new").expect("writing the new binary");
        swap_aside(&exe, &fresh, &old).expect("swapped");
        assert_eq!(fs::read(&exe).expect("reading exe"), b"new");
        assert_eq!(fs::read(&old).expect("reading old"), b"running");
    }

    #[test]
    fn a_failed_swap_puts_the_old_binary_back() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let exe = dir.path().join("beep-node.exe");
        let old = aside(&exe);
        fs::write(&exe, b"running").expect("writing the old binary");
        // No staged copy, so the second rename fails.
        let missing = staged(&exe);
        assert!(swap_aside(&exe, &missing, &old).is_err());
        assert_eq!(fs::read(&exe).expect("reading exe"), b"running");
        assert!(!old.exists(), "the old binary went back");
    }
}
