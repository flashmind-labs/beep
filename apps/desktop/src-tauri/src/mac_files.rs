//! Answers Beep's questions about files on this Mac: lists a folder, finds
//! files by name and reads one, anywhere under the home folder.

use base64::{Engine, engine::general_purpose::STANDARD};
use beep_proto::{DeviceAnswer, MAC_READ, MacFileCommand};
use serde::Serialize;
use serde_json::json;
use std::{
    cmp::Reverse,
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

/// Most entries a list or a search answers with.
const MAX_ENTRIES: usize = 200;
const MAX_FOUND: usize = 50;
/// How many entries a search walks when Spotlight cannot answer.
const MAX_WALKED: usize = 50_000;

/// One entry of a folder.
#[derive(Serialize)]
struct Entry {
    name: String,
    path: String,
    folder: bool,
    size: u64,
    /// UTC, such as "2026-10-19 14:02".
    modified: String,
}

/// Runs `command` off the async threads, since it reads the disk.
pub async fn answer(command: MacFileCommand) -> DeviceAnswer {
    let done = tokio::task::spawn_blocking(move || match home() {
        Some(home) => run(&home, command),
        None => Err("this Mac has no home folder".to_owned()),
    })
    .await;
    match done {
        Ok(Ok(output)) => DeviceAnswer::Ok { output },
        Ok(Err(message)) => DeviceAnswer::Err { message },
        Err(e) => DeviceAnswer::Err {
            message: format!("reading files failed: {e}"),
        },
    }
}

fn home() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .and_then(|home| home.canonicalize().ok())
}

fn run(home: &Path, command: MacFileCommand) -> Result<String, String> {
    match command {
        MacFileCommand::List { path } => list(home, &path),
        MacFileCommand::Find { name, folder } => find(home, &name, &folder),
        MacFileCommand::Read { path } => read(home, &path),
    }
}

/// `path` as a real path under `home`: `~` and relative paths start at
/// home, and links are followed before checking, so none leads out.
fn resolve(home: &Path, path: &str) -> Result<PathBuf, String> {
    let path = path.trim();
    let joined = match path.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ if Path::new(path).is_absolute() => PathBuf::from(path),
        _ => home.join(path),
    };
    if joined.components().any(|c| c == Component::ParentDir) {
        return Err(format!("{path}: use a path without .."));
    }
    let real = joined
        .canonicalize()
        .map_err(|_| format!("{path}: no such file or folder"))?;
    if !real.starts_with(home) {
        return Err(format!("{path}: only files in the home folder can be read"));
    }
    Ok(real)
}

/// `path` as the user would write it, from `~`.
fn shown(home: &Path, path: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn hidden(name: &str) -> bool {
    name.starts_with('.')
}

fn list(home: &Path, path: &str) -> Result<String, String> {
    let dir = resolve(home, path)?;
    if !dir.is_dir() {
        return Err(format!("{path} is a file, not a folder; use read"));
    }
    let mut entries: Vec<(SystemTime, Entry)> = fs::read_dir(&dir)
        .map_err(|e| format!("{path}: {e}"))?
        .flatten()
        .filter_map(|item| {
            let name = item.file_name().to_string_lossy().into_owned();
            if hidden(&name) {
                return None;
            }
            let meta = item.metadata().ok()?;
            let modified = meta.modified().unwrap_or(UNIX_EPOCH);
            let entry = Entry {
                path: shown(home, &item.path()),
                folder: meta.is_dir(),
                size: if meta.is_dir() { 0 } else { meta.len() },
                modified: utc(modified),
                name,
            };
            Some((modified, entry))
        })
        .collect();
    // Newest first: what the user means is usually what they touched last.
    entries.sort_by_key(|(modified, _)| Reverse(*modified));
    let total = entries.len();
    let entries: Vec<Entry> = entries
        .into_iter()
        .take(MAX_ENTRIES)
        .map(|(_, e)| e)
        .collect();
    Ok(json!({ "folder": shown(home, &dir), "total": total, "entries": entries }).to_string())
}

fn find(home: &Path, name: &str, folder: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("give part of a file name to find".to_owned());
    }
    let dir = resolve(home, folder)?;
    let found = match spotlight(&dir, name) {
        Some(found) if !found.is_empty() => found,
        _ => walk(&dir, &name.to_lowercase()),
    };
    let paths: Vec<String> = found
        .iter()
        .filter(|path| path.starts_with(home))
        .filter(|path| {
            !path
                .components()
                .any(|c| hidden(&c.as_os_str().to_string_lossy()))
        })
        .take(MAX_FOUND)
        .map(|path| shown(home, path))
        .collect();
    Ok(json!({ "folder": shown(home, &dir), "found": paths }).to_string())
}

/// Spotlight's matches by name, or `None` when it cannot answer.
fn spotlight(dir: &Path, name: &str) -> Option<Vec<PathBuf>> {
    let out = Command::new("/usr/bin/mdfind")
        .arg("-onlyin")
        .arg(dir)
        .arg("-name")
        .arg(name)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .take(MAX_FOUND * 4)
            .map(PathBuf::from)
            .collect(),
    )
}

/// Matches by walking `dir`, skipping hidden folders and `~/Library`.
fn walk(dir: &Path, needle: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut queue = vec![dir.to_path_buf()];
    let mut walked = 0;
    while let Some(next) = queue.pop() {
        let Ok(items) = fs::read_dir(&next) else {
            continue;
        };
        for item in items.flatten() {
            walked += 1;
            if walked > MAX_WALKED || found.len() >= MAX_FOUND {
                return found;
            }
            let name = item.file_name().to_string_lossy().to_lowercase();
            if hidden(&name) {
                continue;
            }
            let path = item.path();
            if name.contains(needle) {
                found.push(path.clone());
            }
            let is_dir = item.file_type().is_ok_and(|t| t.is_dir());
            if is_dir && !(name == "library" && next == dir) {
                queue.push(path);
            }
        }
    }
    found
}

fn read(home: &Path, path: &str) -> Result<String, String> {
    let file = resolve(home, path)?;
    let meta = fs::metadata(&file).map_err(|e| format!("{path}: {e}"))?;
    if meta.is_dir() {
        return Err(format!("{path} is a folder; use list"));
    }
    if meta.len() > MAC_READ {
        return Err(format!(
            "{path} is {:.1} MB; Beep can read files up to {} MB",
            meta.len() as f64 / 1_048_576.0,
            MAC_READ >> 20
        ));
    }
    let data = fs::read(&file).map_err(|e| format!("{path}: {e}"))?;
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_owned());
    Ok(json!({
        "name": name,
        "path": shown(home, &file),
        "mime": mime(&name),
        "size": data.len(),
        "data": STANDARD.encode(&data),
    })
    .to_string())
}

/// The type a file's extension names, empty when unknown.
pub fn mime(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "html" | "htm" => "text/html",
        "json" => "application/json",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "ics" => "text/calendar",
        "vcf" => "text/vcard",
        "rs" | "py" | "js" | "ts" | "swift" | "kt" | "go" | "c" | "h" | "sh" | "sql" | "css" => {
            "text/plain"
        }
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "doc" => "application/msword",
        "xls" => "application/vnd.ms-excel",
        "pages" => "application/vnd.apple.pages",
        "numbers" => "application/vnd.apple.numbers",
        "key" => "application/vnd.apple.keynote",
        "zip" => "application/zip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "",
    }
}

/// `time` in UTC as "YYYY-MM-DD HH:MM".
fn utc(time: SystemTime) -> String {
    let secs = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's days-to-civil.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        rest / 3_600,
        rest % 3_600 / 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_home(tag: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("beep-mac-files-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("Desktop")).expect("the temp folder is writable");
        dir.canonicalize().expect("the temp folder exists")
    }

    #[test]
    fn paths_stay_in_the_home_folder() {
        let home = temp_home("resolve");
        fs::write(home.join("Desktop/a.txt"), "hi").expect("writable");
        assert_eq!(
            resolve(&home, "~/Desktop/a.txt").unwrap(),
            home.join("Desktop/a.txt")
        );
        assert_eq!(resolve(&home, "Desktop").unwrap(), home.join("Desktop"));
        assert!(resolve(&home, "~/../etc").is_err());
        assert!(resolve(&home, "/etc/hosts").is_err());
        assert!(resolve(&home, "~/missing.txt").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc", home.join("out")).expect("links work");
            assert!(resolve(&home, "~/out/hosts").is_err());
        }
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn lists_newest_first_without_hidden_files() {
        let home = temp_home("list");
        fs::write(home.join("Desktop/old.txt"), "a").expect("writable");
        fs::write(home.join("Desktop/.secret"), "a").expect("writable");
        std::thread::sleep(Duration::from_millis(20));
        fs::write(home.join("Desktop/new.pdf"), "abc").expect("writable");
        let out: serde_json::Value =
            serde_json::from_str(&list(&home, "~/Desktop").unwrap()).unwrap();
        assert_eq!(out["total"], 2);
        assert_eq!(out["entries"][0]["name"], "new.pdf");
        assert_eq!(out["entries"][0]["path"], "~/Desktop/new.pdf");
        assert_eq!(out["entries"][0]["size"], 3);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn reads_a_file_as_base64() {
        let home = temp_home("read");
        fs::write(home.join("Desktop/notes.md"), "# hi").expect("writable");
        let out: serde_json::Value =
            serde_json::from_str(&read(&home, "~/Desktop/notes.md").unwrap()).unwrap();
        assert_eq!(out["mime"], "text/markdown");
        assert_eq!(
            STANDARD.decode(out["data"].as_str().unwrap()).unwrap(),
            b"# hi"
        );
        assert!(read(&home, "~/Desktop").is_err());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn walking_finds_by_part_of_the_name() {
        let home = temp_home("walk");
        fs::create_dir_all(home.join("Documents/Leases")).expect("writable");
        fs::write(home.join("Documents/Leases/Lease 2026.pdf"), "x").expect("writable");
        let found = walk(&home, "lease 2026");
        assert_eq!(found, vec![home.join("Documents/Leases/Lease 2026.pdf")]);
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn dates_read_as_utc() {
        let at = UNIX_EPOCH + Duration::from_secs(1_760_882_520);
        assert_eq!(utc(at), "2025-10-19 14:02");
    }
}
