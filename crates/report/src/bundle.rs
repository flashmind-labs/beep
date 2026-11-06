//! The report draft (the manifest and each machine's redacted files) and
//! the zip that is sent.

use crate::collect::{Answer, Share, newest_lines};
use crate::period::Period;
use crate::redact::{RedactError, redact};
use beep_mesh::DeviceKind;
use beep_proto::report::MAX_BODY;
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read, Write};
use thiserror::Error;
use zip::result::ZipError;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

const MANIFEST: &str = "manifest.json";
/// Room left in `MAX_BODY` for the manifest.
const MANIFEST_RESERVE: usize = 64 * 1024;

/// What the report holds, written first in the zip as `manifest.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub sender: EndpointId,
    pub mesh: Option<EndpointId>,
    /// "desktop" or "node".
    pub app: String,
    pub app_version: String,
    pub text: String,
    pub period: Period,
    /// Unix seconds.
    pub since: i64,
    /// Unix seconds.
    pub made_at: i64,
    pub machines: Vec<MachineEntry>,
    pub files: Vec<FileEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MachineEntry {
    pub id: EndpointId,
    pub name: String,
    pub kind: DeviceKind,
    pub answer: Answer,
    pub version: String,
    pub os: String,
    pub os_version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub bytes: u64,
}

/// A report before it is zipped, for the user to look over.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Draft {
    pub manifest: Manifest,
    pub files: Vec<DraftFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DraftFile {
    pub path: String,
    pub text: String,
}

/// Who sends the report.
pub struct Sender {
    pub id: EndpointId,
    pub mesh: Option<EndpointId>,
    pub app: &'static str,
    pub app_version: String,
}

#[derive(Debug, Error)]
pub enum BundleError {
    #[error(transparent)]
    Redact(#[from] RedactError),
    #[error("the report did not zip: {0}")]
    Zip(#[from] ZipError),
    #[error("the manifest did not read: {0}")]
    Json(#[from] serde_json::Error),
    #[error("the report did not write: {0}")]
    Io(#[from] std::io::Error),
    #[error("the report has no manifest")]
    NoManifest,
    #[error("the report is {bytes} bytes, over the {MAX_BODY} the server takes")]
    TooLarge { bytes: usize },
}

/// The redacted draft of a report from each machine's share, cut to fit
/// under `MAX_BODY`.
pub fn draft(
    text: &str,
    period: Period,
    since: i64,
    sender: Sender,
    shares: Vec<Share>,
) -> Result<Draft, BundleError> {
    let mut machines = Vec::with_capacity(shares.len());
    let mut files = Vec::new();
    for share in shares {
        let dir = folder(&share.name, share.id);
        for (name, body) in [("log.txt", &share.log), ("status.txt", &share.status)] {
            let text = redact(body)?;
            if !text.is_empty() {
                files.push(DraftFile {
                    path: format!("{dir}/{name}"),
                    text,
                });
            }
        }
        machines.push(MachineEntry {
            id: share.id,
            name: share.name,
            kind: share.kind,
            answer: share.answer,
            version: share.version,
            os: share.os,
            os_version: share.os_version,
        });
    }
    fit(&mut files, MAX_BODY - MANIFEST_RESERVE);
    files.retain(|f| !f.text.is_empty());
    let manifest = Manifest {
        version: 1,
        sender: sender.id,
        mesh: sender.mesh,
        app: sender.app.to_owned(),
        app_version: sender.app_version,
        text: redact(text)?,
        period,
        since,
        made_at: chrono::Utc::now().timestamp(),
        machines,
        files: entries(&files),
    };
    Ok(Draft { manifest, files })
}

/// The zip of `draft`, every file redacted again: `manifest.json`, then the files.
pub fn zip(draft: &Draft) -> Result<Vec<u8>, BundleError> {
    let mut files = Vec::with_capacity(draft.files.len());
    for file in &draft.files {
        files.push(DraftFile {
            path: file.path.clone(),
            text: redact(&file.text)?,
        });
    }
    let mut manifest = draft.manifest.clone();
    manifest.text = redact(&manifest.text)?;
    manifest.files = entries(&files);

    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file(MANIFEST, options)?;
    writer.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    for file in &files {
        writer.start_file(file.path.as_str(), options)?;
        writer.write_all(file.text.as_bytes())?;
    }
    let body = writer.finish()?.into_inner();
    if body.len() > MAX_BODY {
        return Err(BundleError::TooLarge { bytes: body.len() });
    }
    Ok(body)
}

/// The manifest of a report zip.
pub fn read_manifest(zip: &[u8]) -> Result<Manifest, BundleError> {
    let mut archive = ZipArchive::new(Cursor::new(zip))?;
    let file = match archive.by_name(MANIFEST) {
        Ok(file) => file,
        Err(ZipError::FileNotFound) => return Err(BundleError::NoManifest),
        Err(e) => return Err(e.into()),
    };
    // Reads no more than a whole report could hold, whatever the zip claims.
    let mut json = Vec::new();
    file.take(MAX_BODY as u64).read_to_end(&mut json)?;
    Ok(serde_json::from_slice(&json)?)
}

/// `<name>-<first 6 of id>`, with nothing in the name that would read as a
/// folder or a drive in the zip.
fn folder(name: &str, id: EndpointId) -> String {
    let mut clean: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .collect();
    while clean.contains("..") {
        clean = clean.replace("..", "-");
    }
    let dots = clean.len() - clean.trim_start_matches('.').len();
    clean.replace_range(..dots, &"-".repeat(dots));
    let id = id.to_string();
    format!("{clean}-{}", &id[..6])
}

/// Cuts `files` to `budget` bytes in all, smallest first: each takes at
/// most an even share of what is left, and a cut keeps its newest lines.
fn fit(files: &mut [DraftFile], budget: usize) {
    let mut order: Vec<usize> = (0..files.len()).collect();
    order.sort_by_key(|&i| files[i].text.len());
    let mut left = budget;
    for (done, &i) in order.iter().enumerate() {
        let share = left / (order.len() - done);
        let text = std::mem::take(&mut files[i].text);
        files[i].text = newest_lines(text, share);
        left -= files[i].text.len();
    }
}

fn entries(files: &[DraftFile]) -> Vec<FileEntry> {
    files
        .iter()
        .map(|f| FileEntry {
            path: f.path.clone(),
            bytes: f.text.len() as u64,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn sender() -> Sender {
        Sender {
            id: SecretKey::generate().public(),
            mesh: None,
            app: "desktop",
            app_version: "0.2.0".into(),
        }
    }

    fn share(name: &str, log: String, status: &str) -> Share {
        Share {
            id: SecretKey::generate().public(),
            name: name.into(),
            kind: DeviceKind::Node,
            answer: Answer::Answered,
            log,
            status: status.into(),
            version: "0.1.0".into(),
            os: "linux".into(),
            os_version: "6.1".into(),
        }
    }

    fn big_log(n: usize) -> String {
        let mut log = String::new();
        let mut line = 0;
        while log.len() < 1024 * 1024 {
            log.push_str(&format!("log {n} line {line} with some words in it\n"));
            line += 1;
        }
        log
    }

    #[test]
    fn a_draft_redacts_logs_status_and_text() {
        let shares = vec![share(
            "build-box",
            "GET /x?token=abc&page=2\n".into(),
            "Authorization: Bearer abc.def\n",
        )];
        let d = draft(
            "my key is ghp_abcdefghijklmnopqrstuvwx",
            Period::Today,
            0,
            sender(),
            shares,
        )
        .expect("a draft of plain text builds");
        assert_eq!(d.manifest.text, "my key is [redacted]");
        assert_eq!(d.files.len(), 2);
        for file in &d.files {
            assert!(file.text.contains("[redacted]"), "{}", file.text);
            assert!(!file.text.contains("abc"), "{}", file.text);
        }
        assert!(d.files[0].path.starts_with("build-box-"));
        assert!(d.files[0].path.ends_with("/log.txt"));
        assert_eq!(d.manifest.files[0].bytes, d.files[0].text.len() as u64);
    }

    #[test]
    fn a_draft_leaves_out_empty_files() {
        let d = draft(
            "",
            Period::Hour,
            0,
            sender(),
            vec![share("gpu-box", String::new(), "")],
        )
        .expect("an empty share drafts");
        assert!(d.files.is_empty());
        assert_eq!(d.manifest.machines.len(), 1);
    }

    #[test]
    fn a_draft_stays_under_the_bundle_cap() {
        let shares = (0..6).map(|n| share("build-box", big_log(n), "")).collect();
        let d = draft("", Period::Day, 0, sender(), shares).expect("six logs draft");
        let total: usize = d.files.iter().map(|f| f.text.len()).sum();
        assert!(total <= MAX_BODY - MANIFEST_RESERVE, "{total}");
        assert!(total > MAX_BODY - MANIFEST_RESERVE - 6 * 64, "{total}");
        assert_eq!(d.files.len(), 6);
        for (n, file) in d.files.iter().enumerate() {
            let original = big_log(n);
            let last = original.lines().last().expect("the log has lines");
            assert!(file.text.ends_with(&format!("{last}\n")));
            assert!(file.text.starts_with(&format!("log {n} line ")));
        }
    }

    #[test]
    fn a_small_file_keeps_all_of_itself_and_leaves_the_rest_to_others() {
        let mut files = vec![
            DraftFile {
                path: "a".into(),
                text: "x\n".repeat(400),
            },
            DraftFile {
                path: "b".into(),
                text: "y\n".repeat(10),
            },
        ];
        fit(&mut files, 300);
        assert_eq!(files[1].text.len(), 20);
        assert_eq!(files[0].text.len(), 280);
    }

    #[test]
    fn a_machine_name_cannot_leave_its_folder() {
        let id = SecretKey::generate().public();
        let short = &id.to_string()[..6];
        assert_eq!(folder("../..\\c:/x", id), format!("----c--x-{short}"));
        assert_eq!(folder(".hidden", id), format!("-hidden-{short}"));
    }

    #[test]
    fn zip_round_trips_the_manifest() {
        let shares = vec![share("build-box", "started\n".into(), "running\n")];
        let d = draft("it hangs", Period::Today, 1_700_000_000, sender(), shares)
            .expect("a plain draft builds");
        let body = zip(&d).expect("a small draft zips");
        let back = read_manifest(&body).expect("the zip has its manifest");
        assert_eq!(
            serde_json::to_value(&back).expect("a manifest is JSON"),
            serde_json::to_value(&d.manifest).expect("a manifest is JSON")
        );
    }

    #[test]
    fn zip_redacts_a_draft_changed_after_drafting() {
        let mut d = draft("", Period::Today, 0, sender(), Vec::new()).expect("an empty draft");
        d.files.push(DraftFile {
            path: "build-box-abcdef/log.txt".into(),
            text: "sent Bearer abc123XYZ now\n".into(),
        });
        let body = zip(&d).expect("the draft zips");
        let mut archive = ZipArchive::new(Cursor::new(body)).expect("a zip reads back");
        let mut text = String::new();
        archive
            .by_name("build-box-abcdef/log.txt")
            .expect("the file is in the zip")
            .read_to_string(&mut text)
            .expect("the file reads");
        assert_eq!(text, "sent Bearer [redacted] now\n");
    }

    #[test]
    fn a_zip_without_a_manifest_says_so() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("x.txt", SimpleFileOptions::default())
            .expect("a file starts");
        let body = writer.finish().expect("the zip ends").into_inner();
        assert!(matches!(read_manifest(&body), Err(BundleError::NoManifest)));
    }
}
