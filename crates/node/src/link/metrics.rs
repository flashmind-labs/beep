//! Bounded summaries across complete snapshots, including daily rotations.

use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    metrics::{MAX_PERIOD, Query, Reply, Request},
};
use chrono::{DateTime, Duration as ChronoDuration, FixedOffset};
use regex::{Regex, RegexBuilder};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
};

const MAX_SCAN: u64 = 256 * 1024 * 1024;
const MAX_SNAPSHOT: usize = 1024 * 1024;
const MAX_SERIES: usize = 2048;
const MAX_REPLY: usize = 256 * 1024;

pub(super) async fn serve(mut tx: LinkTx, mut rx: LinkRx, path: PathBuf) -> Result<()> {
    let Some(request) = rx.recv::<Request>().await? else {
        return Ok(());
    };
    let reply = match tokio::task::spawn_blocking(move || summarize(&path, &request)).await? {
        Ok(text) => Reply::Summary { text },
        Err(e) => Reply::Failed {
            reason: format!("{e:#}"),
        },
    };
    tx.send(&reply).await?;
    tx.finish().await
}

pub(super) async fn serve_query(mut tx: LinkTx, mut rx: LinkRx, path: PathBuf) -> Result<()> {
    let Some(query) = rx.recv::<Query>().await? else {
        return Ok(());
    };
    let reply = match tokio::task::spawn_blocking(move || summarize_query(&path, &query)).await? {
        Ok(text) => Reply::Summary { text },
        Err(e) => Reply::Failed {
            reason: format!("{e:#}"),
        },
    };
    tx.send(&reply).await?;
    tx.finish().await
}

struct Selection {
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
    pattern: Option<Regex>,
    limit: usize,
    offset: usize,
}

impl TryFrom<&Query> for Selection {
    type Error = anyhow::Error;

    fn try_from(query: &Query) -> Result<Self> {
        let start = DateTime::parse_from_rfc3339(&query.start)?;
        let end = DateTime::parse_from_rfc3339(&query.end)?;
        anyhow::ensure!(
            start.offset().local_minus_utc() == 0 && end.offset().local_minus_utc() == 0,
            "start and end must be UTC"
        );
        anyhow::ensure!(
            start.timestamp() >= 0
                && start < end
                && end.signed_duration_since(start) <= ChronoDuration::days(7),
            "start must be on or after 1970 and precede end by at most 7 days"
        );
        anyhow::ensure!(
            (1..=2048).contains(&query.limit),
            "limit must be between 1 and 2048"
        );
        anyhow::ensure!(query.offset <= 2048, "offset must be at most 2048");
        anyhow::ensure!(
            !query.regex || query.search.is_some(),
            "regex requires search"
        );
        let pattern = query
            .search
            .as_ref()
            .map(|search| {
                anyhow::ensure!(search.len() <= 4096, "search must be at most 4096 bytes");
                let pattern = if query.regex {
                    search.clone()
                } else {
                    regex::escape(search)
                };
                Ok::<_, anyhow::Error>(
                    RegexBuilder::new(&pattern)
                        .size_limit(1024 * 1024)
                        .build()?,
                )
            })
            .transpose()?;

        Ok(Self {
            start,
            end,
            pattern,
            limit: usize::try_from(query.limit)?,
            offset: usize::try_from(query.offset)?,
        })
    }
}

fn summarize_query(path: &Path, query: &Query) -> Result<String> {
    let selection = Selection::try_from(query)?;
    let request = Request {
        since: u64::try_from(selection.start.timestamp())?,
        until: u64::try_from(selection.end.timestamp())?,
    };
    summarize_selected(path, &request, Some(&selection))
}

#[derive(Default)]
struct Series {
    first: f64,
    last: f64,
    min: f64,
    max: f64,
    increase: f64,
    samples: usize,
    resets: usize,
}

impl Series {
    fn record(&mut self, value: f64) {
        if self.samples == 0 {
            self.first = value;
            self.min = value;
            self.max = value;
        } else {
            if value < self.last {
                self.resets += 1;
            }
            self.increase += if value >= self.last {
                value - self.last
            } else {
                value
            };
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }

        self.last = value;
        self.samples += 1;
    }
}

fn summarize(path: &Path, request: &Request) -> Result<String> {
    if request.until <= request.since || request.until - request.since > MAX_PERIOD {
        bail!("The metrics period must be positive and at most 7d");
    }

    summarize_selected(path, request, None)
}

fn summarize_selected(
    path: &Path,
    request: &Request,
    selection: Option<&Selection>,
) -> Result<String> {
    let parent = path.parent().context("metrics path has no directory")?;
    let mut paths = Vec::new();

    for entry in std::fs::read_dir(parent).context("reading the metrics directory")? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with("metrics.") && name.ends_with(".log") && name != "metrics.log" {
            paths.push(entry.path());
        }
    }

    paths.sort();
    paths.push(path.to_owned());
    let mut remaining = MAX_SCAN;
    let mut series = BTreeMap::<String, Series>::new();
    let mut counters = BTreeSet::new();
    let mut first = None;
    let mut last = None;
    let mut snapshots = 0;

    for file in paths {
        let file = match File::open(&file) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).context("opening a metrics file"),
        };
        let size = file.metadata()?.len();

        if size > remaining {
            bail!("Retained metrics exceed the 256 MiB scan limit");
        }
        remaining -= size;
        let mut snapshot = String::new();
        let mut time = None;
        // A writer may append while this runs; read only the captured size.
        for line in BufReader::new(file.take(size)).lines() {
            let line = line?;
            if let Some(stamp) = line.strip_prefix("# snapshot unix_seconds=") {
                snapshot.clear();
                time = stamp
                    .parse::<u64>()
                    .ok()
                    .filter(|t| *t >= request.since && *t <= request.until)
                    .filter(|t| {
                        selection.is_none_or(|selection| {
                            i64::try_from(*t)
                                .ok()
                                .and_then(|time| DateTime::from_timestamp(time, 0))
                                .is_some_and(|time| time >= selection.start && time < selection.end)
                        })
                    });
            } else if line == "# end snapshot" {
                if let Some(time) = time.take() {
                    // A repeated or out-of-order timestamp must not look like a counter reset.
                    if last.is_some_and(|last| time <= last) {
                        continue;
                    }

                    first.get_or_insert(time);
                    last = Some(time);
                    snapshots += 1;

                    for line in snapshot.lines() {
                        if let Some(typed) = line.strip_prefix("# TYPE ") {
                            if let Some(name) = typed.strip_suffix(" counter") {
                                counters.insert(name.to_owned());
                            }
                            continue;
                        }

                        if line.starts_with('#') {
                            continue;
                        }

                        let Some((key, value)) = line.rsplit_once(' ') else {
                            continue;
                        };
                        let Ok(value) = value.parse::<f64>() else {
                            continue;
                        };

                        if !value.is_finite() {
                            continue;
                        }

                        if selection.is_some_and(|selection| {
                            selection
                                .pattern
                                .as_ref()
                                .is_some_and(|pattern| !pattern.is_match(key))
                        }) {
                            continue;
                        }

                        if !series.contains_key(key) && series.len() >= MAX_SERIES {
                            bail!("Metrics exceed the 2048 series limit");
                        }

                        series.entry(key.to_owned()).or_default().record(value);
                    }
                }

                snapshot.clear();
            } else if time.is_some() {
                if snapshot.len() + line.len() + 1 > MAX_SNAPSHOT {
                    bail!("A metrics snapshot exceeds 1 MiB");
                }

                snapshot.push_str(&line);
                snapshot.push('\n');
            }
        }
    }

    let (Some(first), Some(last)) = (first, last) else {
        return Ok("No complete metrics snapshots in this period. Metrics may be unavailable or outside retention.\n".to_owned());
    };
    let requested = match selection {
        Some(selection) => format!(
            "UTC [{}, {})",
            selection.start.to_rfc3339(),
            selection.end.to_rfc3339()
        ),
        None => format!("Unix seconds {}..{}", request.since, request.until),
    };
    let mut text = format!(
        "Requested {requested}; observed {first}..{last}, {snapshots} complete snapshots.\nCounter increases cover observed samples only; activity before the first sample and between restarts can be missing.\nQuantile ranges are sampled exporter-window quantiles, not whole-period percentiles.\n"
    );

    let total = series.len();
    let (offset, limit) =
        selection.map_or((0, total), |selection| (selection.offset, selection.limit));

    if selection.is_some() {
        let returned = total.saturating_sub(offset).min(limit);
        let next = offset + returned;
        let next_offset = if next < total {
            next.to_string()
        } else {
            "none".to_owned()
        };
        text.push_str(&format!("Matching series: {total}; offset: {offset}; returned: {returned}; next_offset: {next_offset}.\n"));
    }

    for (key, sample) in series.into_iter().skip(offset).take(limit) {
        let name = key.split('{').next().unwrap_or(&key);
        let cumulative = counters.contains(name)
            || name
                .strip_suffix("_total")
                .is_some_and(|base| counters.contains(base))
            || name.ends_with("_sum")
            || name.ends_with("_count")
            || name.ends_with("_bucket");
        let line = if cumulative {
            format!(
                "{key}: increase={} first={} last={} resets={} samples={}\n",
                sample.increase, sample.first, sample.last, sample.resets, sample.samples
            )
        } else {
            format!(
                "{key}: min={} max={} last={} samples={}\n",
                sample.min, sample.max, sample.last, sample.samples
            )
        };

        if text.len() + line.len() > MAX_REPLY {
            if selection.is_some() {
                bail!("Metrics page exceeds 256 KiB; request a smaller limit");
            }

            text.push_str("Output truncated at 256 KiB; additional series omitted.\n");
            break;
        }

        text.push_str(&line);
    }

    Ok(text)
}

#[cfg(test)]
#[path = "../tests/link_metrics.rs"]
mod tests;
