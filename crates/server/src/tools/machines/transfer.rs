//! Sending a file or folder between the user's machines; a taken name gets a number, never replaced.

use super::{Picked, copy};
use crate::{
    nodes::{
        TRANSFER,
        transfer::{Landed, Progress},
    },
    protocol::node::{
        files::{FileKind, Reply, Request},
        transfer::{ContentKind, MAX_SUFFIX, Stage, numbered},
    },
};
use anyhow::{Context, Result, bail, ensure};
use tokio_util::sync::CancellationToken;

/// What a send did.
#[derive(Debug, PartialEq)]
pub(crate) struct Sent {
    /// Where it landed on the target.
    pub(crate) path: String,
    pub(crate) size: u64,
    pub(crate) files: u64,
    /// Symlinks and special files a folder held that were left out.
    pub(crate) skipped: u64,
    /// Whether it went node to node, rather than through the server.
    pub(crate) direct: bool,
}

/// A folder on a machine, resolved against `~` or `base`; the result must be absolute.
pub(crate) fn folder_on_machine(
    folder: &str,
    base: Option<&str>,
    home: Option<&str>,
) -> Result<String> {
    let folder = folder.trim();
    let joined = |dir: &str, rest: &str| format!("{}/{rest}", dir.trim_end_matches('/'));
    let resolved = match folder {
        "" | "~" => home.map(str::to_owned),
        _ => match folder.strip_prefix("~/") {
            Some(rest) => home.map(|home| joined(home, rest)),
            None if folder.starts_with('/') => Some(folder.to_owned()),
            None => base.or(home).map(|dir| joined(dir, folder)),
        },
    };
    let resolved = resolved.with_context(|| {
        format!("the machine has not reported its home folder, so give an absolute folder instead of {folder:?}")
    })?;
    ensure!(
        resolved.starts_with('/'),
        "{resolved} is not an absolute folder"
    );
    Ok(resolved)
}

/// Turns node progress into a line per 5 percent of each stage.
struct Reporter<'a> {
    stage: Option<Stage>,
    last: Option<u64>,
    out: &'a mut (dyn FnMut(String) + Send),
}

impl<'a> Reporter<'a> {
    fn new(out: &'a mut (dyn FnMut(String) + Send)) -> Self {
        Self {
            stage: None,
            last: None,
            out,
        }
    }

    fn report(&mut self, progress: Progress) {
        if self.stage != Some(progress.stage) {
            self.stage = Some(progress.stage);
            self.last = None;
        }
        // In u128, so no count overflows.
        let percent = (u128::from(progress.done) * 100)
            .checked_div(u128::from(progress.total))
            .map_or(100, |percent| u64::try_from(percent).unwrap_or(100))
            .min(100);
        if self.last.is_some_and(|last| percent < last + 5) {
            return;
        }
        self.last = Some(percent);
        (self.out)(format!(
            "{} {} of {} ({percent}%)",
            doing(progress.stage),
            size(progress.done),
            size(progress.total)
        ));
    }
}

/// What a stage's progress line starts with.
fn doing(stage: Stage) -> &'static str {
    match stage {
        Stage::Hashing => "Hashing",
        Stage::Downloading => "Sending",
        Stage::Writing => "Writing out",
        Stage::Other => "Working",
    }
}

/// A byte count for people: `512 B`, `3.4 MiB`, `1.2 GiB`.
pub(crate) fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Sends `path` on `source` into `folder` on `target`; on `cancel` the target removes what it wrote.
pub(crate) async fn send(
    source: &Picked,
    path: &str,
    target: &Picked,
    folder: &str,
    progress: &mut (dyn FnMut(String) + Send),
    cancel: &CancellationToken,
) -> Result<Sent> {
    let target_key = target.online.connection.remote_id();
    ensure!(
        source.online.connection.remote_id() != target_key,
        "{} is both the source and the target; copy it there with exec",
        source.shown()
    );
    if !(source.online.offers(TRANSFER) && target.online.offers(TRANSFER)) {
        return copy_through_server(source, path, target, folder).await;
    }
    let mut reporter = Reporter::new(progress);
    let offer = source
        .online
        .offer(path, target_key, &mut |p| reporter.report(p))
        .await?;
    let fetched = target
        .online
        .fetch(&offer.offered, folder, &mut |p| reporter.report(p), cancel)
        .await;
    let skipped = offer.offered.skipped;
    if let Err(e) = offer.close().await {
        // The grant goes with the stream either way.
        log::debug!("an offer did not close cleanly: {e:#}");
    }
    let Landed { path, size, files } = fetched?;
    Ok(Sent {
        path,
        size,
        files,
        skipped,
        direct: true,
    })
}

/// The `files/1` copy for nodes without `transfer/1`: one file, to the first free name.
async fn copy_through_server(
    source: &Picked,
    path: &str,
    target: &Picked,
    folder: &str,
) -> Result<Sent> {
    let name = copy::file_name(path).context("the path names no file")?;
    let folder = folder.trim_end_matches('/');
    // As on the direct path, the folder must be there already.
    let found = target
        .online
        .files(&Request::Stat {
            path: if folder.is_empty() { "/" } else { folder }.to_owned(),
        })
        .await?;
    match found {
        Reply::Stat { entry, .. } if entry.kind == FileKind::Dir => {}
        Reply::Stat { .. } => bail!("{folder} on {} is not a folder", target.shown()),
        Reply::Error { message } => bail!("the target folder {folder}: {message}"),
        other => bail!("{} answered {other:?}", target.shown()),
    }
    for n in 1..=MAX_SUFFIX {
        let dst = format!("{folder}/{}", numbered(name, ContentKind::File, n));
        let taken = matches!(
            target
                .online
                .files(&Request::Stat { path: dst.clone() })
                .await?,
            Reply::Stat { .. }
        );
        if taken {
            continue;
        }
        let copied = copy::copy(&source.online, path, &target.online, &dst, false)
            .await
            .map_err(|e| {
                e.context(format!(
                    "{} and {} need a newer beep-node to send folders or send directly",
                    source.shown(),
                    target.shown()
                ))
            })?;
        return Ok(Sent {
            path: copied.path,
            size: copied.bytes,
            files: 1,
            skipped: 0,
            direct: false,
        });
    }
    bail!("{name} and {MAX_SUFFIX} numbered names after it are taken in {folder}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_resolve_to_absolute_paths() {
        let home = Some("/Users/me");
        assert_eq!(folder_on_machine("~", None, home).unwrap(), "/Users/me");
        assert_eq!(folder_on_machine("", None, home).unwrap(), "/Users/me");
        assert_eq!(
            folder_on_machine("~/Renders", None, home).unwrap(),
            "/Users/me/Renders"
        );
        assert_eq!(
            folder_on_machine("in", Some("/home/user"), home).unwrap(),
            "/home/user/in"
        );
        assert_eq!(folder_on_machine("/srv/x", None, None).unwrap(), "/srv/x");
        assert!(folder_on_machine("in", None, None).is_err());
        assert!(folder_on_machine("~", None, None).is_err());
    }

    #[test]
    fn progress_is_reported_every_five_percent() {
        let mut lines = Vec::new();
        let mut out = |line: String| lines.push(line);
        let mut reporter = Reporter::new(&mut out);
        let at = |stage, done, total| Progress { done, total, stage };
        for done in [0, 1, 4, 5, 7, 50, 100] {
            reporter.report(at(Stage::Downloading, done, 100));
        }
        reporter.report(at(Stage::Downloading, 0, 0));
        // A new stage starts over, and huge counts do not overflow.
        reporter.report(at(Stage::Writing, u64::MAX / 2, u64::MAX));
        assert_eq!(
            lines,
            [
                "Sending 0 B of 100 B (0%)",
                "Sending 5 B of 100 B (5%)",
                "Sending 50 B of 100 B (50%)",
                "Sending 100 B of 100 B (100%)",
                "Writing out 8388608.0 TiB of 16777216.0 TiB (49%)",
            ]
        );
        assert_eq!(size(3 << 20), "3.0 MiB");
    }
}
