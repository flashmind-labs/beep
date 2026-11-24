//! Bounded summaries across complete snapshots, including daily rotations.

use anyhow::{Context, Result, bail};
use beep_proto::node::{
    LinkRx, LinkTx,
    metrics::{MAX_PERIOD, Query, Reply, Request},
};
use chrono::{DateTime, Duration as ChronoDuration, FixedOffset, NaiveDate, SecondsFormat, Utc};
use regex::{Regex, RegexBuilder};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{BufRead, BufReader, Read, Take},
    ops::RangeInclusive,
    path::{Path, PathBuf},
};

const MAX_SCAN: u64 = 256 * 1024 * 1024;
const MAX_SNAPSHOT: usize = 1024 * 1024;
const MAX_SERIES: usize = 2048;
const MAX_REPLY: usize = 256 * 1024;
const SNAPSHOT_HEADER: &str = "# snapshot unix_seconds=";

type MetricsFile = BufReader<Take<File>>;

pub(super) async fn serve(tx: LinkTx, rx: LinkRx, path: PathBuf) -> Result<()> {
    serve_with(tx, rx, path, summarize).await
}

pub(super) async fn serve_query(tx: LinkTx, rx: LinkRx, path: PathBuf) -> Result<()> {
    serve_with(tx, rx, path, summarize_query).await
}

async fn serve_with<T: serde::de::DeserializeOwned + Send + 'static>(
    mut tx: LinkTx,
    mut rx: LinkRx,
    path: PathBuf,
    summarize: fn(&Path, &T) -> Result<String>,
) -> Result<()> {
    let Some(request) = rx.recv::<T>().await? else {
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

/// Whether `name` is a day's rotated file, not the live `metrics.log`.
pub(super) fn is_rotated(name: &str) -> bool {
    name.starts_with("metrics.") && name.ends_with(".log") && name != "metrics.log"
}

fn rotation(name: &str) -> Option<(NaiveDate, u64)> {
    let stem = name.strip_prefix("metrics.")?.strip_suffix(".log")?;
    let (date, number) = match stem.split_once('.') {
        Some((date, number)) => (date, number.parse().ok()?),
        None => (stem, 0),
    };

    Some((NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?, number))
}

fn utc_date(seconds: u64) -> Option<NaiveDate> {
    utc_stamp(seconds).map(|stamp| stamp.date_naive())
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
    // Snapshots have whole-second timestamps. Use inclusive bounds internally.
    let request = Request {
        since: u64::try_from(selection.start.timestamp())?
            + u64::from(selection.start.timestamp_subsec_nanos() != 0),
        until: u64::try_from(selection.end.timestamp())?
            - u64::from(selection.end.timestamp_subsec_nanos() == 0),
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
    if request.since > request.until {
        return Ok(no_snapshots());
    }

    let dates = utc_date(request.since).zip(utc_date(request.until));
    let dates = dates.map(|(start, end)| start..=end);
    let files = metrics_files(path, dates.as_ref())?;
    let mut remaining = MAX_SCAN;
    let mut series = BTreeMap::<String, Series>::new();
    let mut counters = BTreeSet::new();
    let mut first = None;
    let mut last = None;
    let mut snapshots = 0;

    for file in files {
        let size = file.get_ref().limit() + u64::try_from(file.buffer().len())?;

        if size > remaining {
            bail!(
                "Metrics logs for this window exceed the 256 MiB read limit. Try a window spanning fewer UTC dates."
            );
        }
        remaining -= size;
        let mut snapshot = String::new();
        let mut time = None;
        for line in file.lines() {
            let line = line?;
            if let Some(stamp) = line.strip_prefix(SNAPSHOT_HEADER) {
                snapshot.clear();
                time = stamp
                    .parse::<u64>()
                    .ok()
                    .filter(|t| *t >= request.since && *t <= request.until);
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
        return Ok(no_snapshots());
    };
    let requested = match selection {
        Some(selection) => format!(
            "UTC [{}, {})",
            selection.start.to_rfc3339(),
            selection.end.to_rfc3339()
        ),
        None => format!(
            "{} to {} (inclusive)",
            display_stamp(request.since),
            display_stamp(request.until)
        ),
    };
    let mut text = format!(
        "Requested {requested}; observed {} to {}, {snapshots} complete snapshots.\nCounter increases cover observed samples only; activity before the first sample and between restarts can be missing.\nQuantile ranges are sampled exporter-window quantiles, not whole-period percentiles.\n",
        display_stamp(first),
        display_stamp(last)
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

fn no_snapshots() -> String {
    "No complete metrics snapshots in this period. Metrics may be unavailable or outside retention.\n".to_owned()
}

fn utc_stamp(seconds: u64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(i64::try_from(seconds).ok()?, 0)
}

fn display_stamp(seconds: u64) -> String {
    utc_stamp(seconds).map_or_else(
        || seconds.to_string(),
        |stamp| stamp.to_rfc3339_opts(SecondsFormat::Secs, true),
    )
}

fn metrics_files(
    path: &Path,
    dates: Option<&RangeInclusive<NaiveDate>>,
) -> Result<Vec<MetricsFile>> {
    let parent = path.parent().context("metrics path has no directory")?;
    // Open the live file before listing rotations so a rollover cannot hide it.
    let mut current = open_metrics(path)?;
    if let (Some(file), Some(dates)) = (&mut current, dates)
        && first_snapshot_date(file)?.is_some_and(|date| date > *dates.end())
    {
        current = None;
    }

    let mut paths = Vec::new();

    for entry in std::fs::read_dir(parent).context("reading the metrics directory")? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if is_rotated(&name) {
            paths.push((rotation(&name), entry.path()));
        }
    }

    paths.sort();
    let mut files = Vec::new();
    for (rotation, path) in paths {
        if let (Some(dates), Some((date, _))) = (dates, rotation) {
            if date < *dates.start() {
                continue;
            }

            if date > *dates.end() {
                // A snapshot whose export crossed midnight can be at the start of
                // the next day's file.
                if dates.end().succ_opt() == Some(date)
                    && let Some(mut file) = open_metrics(&path)?
                    && first_snapshot_date(&mut file)?.is_some_and(|first| first <= *dates.end())
                {
                    files.push(file);
                }
                continue;
            }
        }

        files.extend(open_metrics(&path)?);
    }
    files.extend(current);

    Ok(files)
}

fn first_snapshot_date(file: &mut MetricsFile) -> Result<Option<NaiveDate>> {
    let date = file
        .fill_buf()?
        .split_inclusive(|&byte| byte == b'\n')
        .next()
        .and_then(|line| line.strip_suffix(b"\n"))
        .and_then(|line| std::str::from_utf8(line).ok())
        .and_then(|line| line.strip_prefix(SNAPSHOT_HEADER))
        .and_then(|stamp| stamp.parse().ok())
        .and_then(utc_date);

    Ok(date)
}

fn open_metrics(path: &Path) -> Result<Option<MetricsFile>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("opening a metrics file"),
    };
    // A writer may append while this runs; read only the captured size.
    let size = file.metadata()?.len();

    Ok(Some(BufReader::new(file.take(size))))
}

#[cfg(test)]
#[path = "../tests/link_metrics.rs"]
mod tests;
