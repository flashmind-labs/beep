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
/// The files a machine's folder holds.
const NAMES: [&str; 2] = ["log.txt", "status.txt"];
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
    #[error("{0} is not a report file")]
    BadPath(String),
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
        for (name, body) in NAMES.into_iter().zip([&share.log, &share.status]) {
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
        if !is_report_file(&file.path) {
            return Err(BundleError::BadPath(file.path.clone()));
        }
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

/// `<name>-<first 6 of id>`.
fn folder(name: &str, id: EndpointId) -> String {
    let id = id.to_string();
    format!("{}-{}", clean(name), &id[..6])
}

/// Whether `path` is `<folder>/log.txt` or `<folder>/status.txt`, as
/// `draft` names them.
fn is_report_file(path: &str) -> bool {
    path.split_once('/')
        .is_some_and(|(dir, name)| !dir.is_empty() && clean(dir) == dir && NAMES.contains(&name))
}

/// `name` with nothing that would read as a folder, a drive or a name
/// Windows refuses.
fn clean(name: &str) -> String {
    let mut clean: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
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
    clean
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
#[path = "tests/bundle.rs"]
mod tests;
