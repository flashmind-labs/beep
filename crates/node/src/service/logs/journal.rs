//! The log of the systemd user unit, read with `journalctl`.

use super::{LogOptions, Printer};
use crate::service::systemd::{UNIT, unit_path};
use anyhow::{Context, Result, bail};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

/// What journalctl prints last with `--show-cursor`: where it stopped.
const CURSOR: &str = "-- cursor: ";

pub(super) fn read(options: &LogOptions, printer: &mut Printer<impl Write>) -> Result<()> {
    if !unit_path()?.exists() {
        bail!("the node's service is not installed here: there is no {UNIT}");
    }
    let since = format!("-{}s", options.since.as_secs());
    // The past first, so `--lines` knows where it ends; following then
    // starts right after its last entry.
    let mut past = journalctl();
    past.args(["--since", &since]);
    if options.follow {
        past.arg("--show-cursor");
    }
    let cursor = stream(past, printer)?;
    printer.caught_up()?;
    if !options.follow {
        return Ok(());
    }
    let mut follow = journalctl();
    follow.arg("--follow");
    match &cursor {
        Some(cursor) => follow.args(["--after-cursor", cursor]),
        None => follow.args(["--since", &since]),
    };
    stream(follow, printer)?;
    Ok(())
}

/// `journalctl` for the unit, printing the node's own lines as written.
fn journalctl() -> Command {
    let mut command = Command::new("journalctl");
    command.args([
        "--user",
        "--unit",
        UNIT,
        "--output",
        "cat",
        "--lines",
        "all",
        "--no-pager",
        "--quiet",
    ]);
    command.stdin(Stdio::null()).stdout(Stdio::piped());
    command
}

/// Runs `command`, passing each line to `printer` as it comes, and returns
/// the cursor journalctl printed last, if asked to.
fn stream(mut command: Command, printer: &mut Printer<impl Write>) -> Result<Option<String>> {
    let mut child = command.spawn().context("running journalctl")?;
    let stdout = child.stdout.take().context("journalctl has no output")?;
    let mut reader = BufReader::new(stdout);
    let mut cursor = None;
    let mut line = Vec::new();
    let read = loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) => break Ok(()),
            Ok(_) => {}
            Err(e) => break Err(e),
        }
        if let Some(rest) = line.strip_prefix(CURSOR.as_bytes()) {
            cursor = Some(String::from_utf8_lossy(rest).trim().to_owned());
            continue;
        }
        // Stdout is line buffered, so a followed line shows at once.
        if let Err(e) = printer.bytes(&line) {
            break Err(e);
        }
    };
    if let Err(e) = read {
        // Stops journalctl when the reader went away, such as a closed pipe.
        // It may have exited already, which is the state wanted.
        let _ = child.kill();
        let _ = child.wait();
        return Err(e.into());
    }
    let status = child.wait().context("waiting for journalctl")?;
    if !status.success() {
        bail!("journalctl failed ({status})");
    }
    Ok(cursor)
}
