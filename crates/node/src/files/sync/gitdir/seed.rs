//! Recovery objects stay local even when a new machine imports this repository.

use super::{GIT, RefNow, Repo};
use crate::files::sync::disk::{Found, Stat};
use anyhow::{Context, Result};
use std::{fs, io::Write, path::Path, process::Stdio};

impl Repo {
    /// Build a bootstrap pack from committed refs, never from stash or recovery refs.
    pub(in crate::files::sync) fn seed_objects(
        &self,
        cache: &Path,
        refs: &[RefNow],
    ) -> Result<Vec<Found>> {
        fs::create_dir_all(cache)?;
        let mut tips: Vec<_> = refs.iter().map(|r| r.oid.to_string()).collect();
        if let Some(head) = self.head() {
            tips.push(head.to_string());
        }
        tips.sort();
        tips.dedup();
        let input = tips.join("\n") + "\n";
        let key = blake3::hash(input.as_bytes()).to_hex().to_string();
        let manifest = cache.join("current.json");
        let cached: Option<(String, String)> = fs::read(&manifest)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok());
        let pack = match cached.filter(|(saved, pack)| {
            saved == &key
                && valid_pack(pack)
                && cache.join(format!("pack-{pack}.pack")).is_file()
                && cache.join(format!("pack-{pack}.idx")).is_file()
        }) {
            Some((_, pack)) => pack,
            None => {
                let mut command = self.git();
                command
                    .args(["pack-objects", "--revs"])
                    .arg(cache.join("pack"))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let mut child = command.spawn()?;
                child
                    .stdin
                    .take()
                    .context("Git pack input is unavailable")?
                    .write_all(input.as_bytes())?;
                let output = child.wait_with_output()?;
                anyhow::ensure!(
                    output.status.success(),
                    "Git could not prepare the committed checkout for import"
                );
                let pack = String::from_utf8(output.stdout)?.trim().to_owned();
                anyhow::ensure!(valid_pack(&pack), "Git returned an invalid pack name");
                crate::files::note::write_json(&manifest, &(key, &pack))?;
                pack
            }
        };
        // Previous packs have been copied into the blob store before their list was published.
        for entry in fs::read_dir(cache)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if let Some((old, ext)) = name.strip_prefix("pack-").and_then(|s| s.rsplit_once('.'))
                && valid_pack(old)
                && old != pack
                && matches!(ext, "pack" | "idx")
            {
                fs::remove_file(entry.path())?;
            }
        }
        ["pack", "idx"]
            .into_iter()
            .map(|ext| {
                let name = format!("pack-{pack}.{ext}");
                let abs = cache.join(&name);
                Ok(Found {
                    path: format!("{GIT}/objects/pack/{name}"),
                    stat: Stat::from(&fs::metadata(&abs)?),
                    abs,
                })
            })
            .collect()
    }
}

fn valid_pack(name: &str) -> bool {
    matches!(name.len(), 40 | 64) && name.bytes().all(|b| b.is_ascii_hexdigit())
}
