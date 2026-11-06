//! The command line on its own, with no node running.

use beep_proto::report::Receipt;
use beep_report::{Answer, Period, read_manifest};
use std::{
    io::{Cursor, ErrorKind, Read, Write},
    net::TcpListener,
    process::{Command, Output, Stdio},
    thread::{self, JoinHandle},
};
use tempfile::TempDir;
use zip::ZipArchive;

fn beep_node(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_beep-node"))
        .args(args)
        .output()
        .expect("beep-node runs");
    assert!(
        out.status.success(),
        "beep-node {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("text")
}

#[test]
fn help_lists_the_commands_in_groups() {
    let help = beep_node(&["--help"]);
    for heading in ["Mesh", "Agents", "Service", "Diagnostics", "Setup"] {
        assert!(
            help.lines().any(|line| line == heading),
            "no {heading} group in:\n{help}"
        );
    }
    for listed in [
        "mesh join",
        "status",
        "[aliases: s, st]",
        "beep-node help <command>",
    ] {
        assert!(help.contains(listed), "{listed} missing from:\n{help}");
    }
}

#[test]
fn status_says_when_no_node_runs_and_how_to_start_one() {
    let dir = TempDir::new().expect("a temp folder");
    let dir = dir.path().to_str().expect("a utf-8 path");
    let text = beep_node(&["status", "--dir", dir]);
    assert!(text.contains("not running"), "{text}");
    assert!(text.contains("beep-node run"), "{text}");
    assert_eq!(beep_node(&["st", "--dir", dir]), text);
}

/// A node folder with a key and settings, as a first `run` leaves it.
fn set_up_dir() -> TempDir {
    let dir = TempDir::new().expect("a temp folder");
    let key = iroh::SecretKey::generate();
    std::fs::write(dir.path().join("key"), key.to_bytes()).expect("the key is written");
    std::fs::write(dir.path().join("node.toml"), "name = \"build-box\"\n")
        .expect("settings are written");
    dir
}

/// A report server on a free local port, and its URL.
fn stand_in() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port binds");
    let addr = listener
        .local_addr()
        .expect("a bound listener has an address");
    (listener, format!("http://{addr}"))
}

/// Answers one post with a receipt and hands back the body it read.
fn take_one(listener: TcpListener) -> JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("the report is posted");
        let mut read = Vec::new();
        let mut chunk = [0u8; 8192];
        let head_end = loop {
            let n = stream.read(&mut chunk).expect("the request reads");
            assert!(n > 0, "the request ended before its headers");
            read.extend_from_slice(&chunk[..n]);
            if let Some(at) = read.windows(4).position(|w| w == b"\r\n\r\n") {
                break at + 4;
            }
        };
        let head = String::from_utf8_lossy(&read[..head_end]).to_lowercase();
        let length: usize = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length: "))
            .expect("a length")
            .trim()
            .parse()
            .expect("a number");
        let mut body = read[head_end..].to_vec();
        while body.len() < length {
            let n = stream.read(&mut chunk).expect("the body reads");
            assert!(n > 0, "the body ended early");
            body.extend_from_slice(&chunk[..n]);
        }
        let reply = rmp_serde::to_vec_named(&Receipt {
            reference: "R-TEST".into(),
        })
        .expect("a receipt encodes");
        let head = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/msgpack\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            reply.len()
        );
        stream.write_all(head.as_bytes()).expect("the reply writes");
        stream.write_all(&reply).expect("the reply writes");
        body
    })
}

/// `beep-node report` on `dir`, sending to `url`, with `stdin` typed in.
fn report(dir: &TempDir, url: &str, args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_beep-node"))
        .arg("--dir")
        .arg(dir.path())
        .arg("report")
        .args(args)
        .env("BEEP_REPORT_URL", url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("beep-node runs");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(stdin.as_bytes())
        .expect("stdin takes it");
    child.wait_with_output().expect("beep-node finishes")
}

fn ran(out: &Output) -> String {
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "report failed: {stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

#[test]
fn report_with_no_node_sends_this_machines_log() {
    let dir = set_up_dir();
    let (listener, url) = stand_in();
    let posted = take_one(listener);
    let args = ["--yes", "--message", "it froze", "--period", "hour"];
    let stdout = ran(&report(&dir, &url, &args, ""));
    assert!(stdout.contains("Sent. Reference: R-TEST"), "{stdout}");
    let body = posted.join().expect("the stand-in ran");
    let manifest = read_manifest(&body).expect("a report zip");
    assert_eq!(manifest.text, "it froze");
    assert_eq!(manifest.period, Period::Hour);
    assert_eq!(manifest.machines.len(), 1, "{:?}", manifest.machines);
    assert_eq!(manifest.machines[0].answer, Answer::Answered);
    let mut zip = ZipArchive::new(Cursor::new(body)).expect("a zip");
    let name = manifest
        .files
        .iter()
        .map(|file| file.path.clone())
        .find(|path| path.ends_with("/status.txt"))
        .expect("a status file");
    let mut status = String::new();
    zip.by_name(&name)
        .expect("the status file is in the zip")
        .read_to_string(&mut status)
        .expect("the status reads");
    assert_eq!(status, "no node runs here");
}

#[test]
fn report_without_yes_sends_nothing_on_no() {
    let dir = set_up_dir();
    let (listener, url) = stand_in();
    let stdout = ran(&report(&dir, &url, &["--message", "it froze"], "n\n"));
    assert!(stdout.contains("Send this to Beep? [y/N]"), "{stdout}");
    assert!(!stdout.contains("Reference:"), "{stdout}");
    listener
        .set_nonblocking(true)
        .expect("the listener goes nonblocking");
    assert!(
        listener
            .accept()
            .is_err_and(|e| e.kind() == ErrorKind::WouldBlock),
        "something was posted"
    );
}
