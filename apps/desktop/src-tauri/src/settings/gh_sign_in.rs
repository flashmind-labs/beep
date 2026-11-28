//! Signing `gh` in to GitHub from Settings: Beep runs `gh auth login --web`,
//! shows the one-time code it prints and opens GitHub's device page. Once
//! the code is entered there gh exits, and its token is saved as "Use gh
//! sign-in" saves it.

use crate::store::lock;
use crate::{
    error::Result,
    fleet::projects::FleetState,
    settings::cli_login::{find_tool, refused, save_gh_login},
    settings::credentials::CredentialView,
};
use anyhow::Context;
use serde::Serialize;
use std::{path::Path, process::Stdio, sync::Mutex, time::Duration};
use tauri::{State, ipc::Channel};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, ChildStderr, ChildStdout, Command},
    select,
    sync::oneshot,
    time::timeout,
};
use url::Url;

/// How long the code may wait to be entered. GitHub's codes last 15 minutes.
const LIMIT: Duration = Duration::from_secs(15 * 60);
/// The longest error line from gh the bar shows.
const REASON_CHARS: usize = 200;

/// The sign-in under way, if any: one at a time.
#[derive(Default)]
pub struct GhSignIn(Mutex<Slot>);

#[derive(Default)]
struct Slot {
    running: Option<Running>,
}

/// Stops one sign-in, and tells when it has stopped.
struct Running {
    /// Taken once the sign-in is told to stop.
    cancel: Option<oneshot::Sender<()>>,
    /// Taken by quitting, to wait for gh to be gone.
    ended: Option<oneshot::Receiver<()>>,
}

/// What a started sign-in holds: Cancel's signal, and a sender it drops
/// once gh is gone.
struct Started {
    cancelled: oneshot::Receiver<()>,
    ended: oneshot::Sender<()>,
}

impl GhSignIn {
    fn start(&self) -> Result<Started> {
        let mut slot = lock(&self.0);

        if let Some(running) = &slot.running {
            return Err(refused(if running.cancel.is_some() {
                "A GitHub sign-in is already under way"
            } else {
                "A GitHub sign-in is still being stopped"
            }));
        }

        let (cancel, cancelled) = oneshot::channel();
        let (ended_tx, ended) = oneshot::channel();
        slot.running = Some(Running {
            cancel: Some(cancel),
            ended: Some(ended),
        });
        Ok(Started {
            cancelled,
            ended: ended_tx,
        })
    }

    /// Clears the slot once gh is gone.
    fn finish(&self) {
        lock(&self.0).running = None;
    }

    /// Tells the sign-in under way to stop, which kills gh. The slot stays
    /// taken until gh is gone and `finish` clears it.
    fn cancel(&self) {
        self.stop(false);
    }

    /// Stops the sign-in under way as Beep quits. The future ends once gh
    /// is killed, so it does not outlive the app; quitting bounds the wait.
    pub fn stop_on_quit(&self) -> impl Future<Output = ()> + Send + 'static {
        let ended = self.stop(true);
        async move {
            // The sender is dropped, never sent on, once gh is gone.
            if let Some(ended) = ended {
                ended.await.ok();
            }
        }
    }

    fn stop(&self, take_ended: bool) -> Option<oneshot::Receiver<()>> {
        let mut slot = lock(&self.0);
        let running = slot.running.as_mut()?;
        // An error means the sign-in had just ended on its own.
        if let Some(cancel) = running.cancel.take()
            && cancel.send(()).is_err()
        {
            log::debug!("the GitHub sign-in had already ended");
        }

        take_ended.then(|| running.ended.take()).flatten()
    }
}

/// Clears the slot however the sign-in ends.
struct Finish<'a>(&'a GhSignIn);

impl Drop for Finish<'_> {
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// The code to enter on GitHub and the page to enter it on.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DeviceCode {
    code: String,
    url: String,
}

/// Runs gh's browser sign-in, sends its code to `on_code`, and saves the
/// token once the code is entered on GitHub.
#[tauri::command]
pub async fn gh_sign_in(
    fleet: FleetState<'_>,
    sign_in: State<'_, GhSignIn>,
    on_code: Channel<DeviceCode>,
) -> Result<CredentialView> {
    let gh = find_tool("gh").ok_or_else(|| refused("gh is not installed on this computer"))?;
    let Started { cancelled, ended } = sign_in.start()?;
    let _finish = Finish(&sign_in);
    run_gh(&gh, &on_code, cancelled).await?;
    // Dropped only now, after gh is gone, so quitting waits for that.
    drop(ended);
    save_gh_login(&fleet).await
}

/// Stops the GitHub sign-in under way.
#[tauri::command]
pub fn cancel_gh_sign_in(sign_in: State<'_, GhSignIn>) {
    sign_in.cancel();
}

/// Runs `gh auth login --web` until it signs in, fails, times out or is
/// cancelled. gh is killed in all but the first two.
async fn run_gh(
    gh: &Path,
    on_code: &Channel<DeviceCode>,
    cancelled: oneshot::Receiver<()>,
) -> Result<()> {
    let mut child = Command::new(gh)
        .args([
            "auth",
            "login",
            "--web",
            "--hostname",
            "github.com",
            "--git-protocol",
            "https",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("starting {}", gh.display()))?;
    let stdout = child.stdout.take().context("gh's output was not piped")?;
    let stderr = child.stderr.take().context("gh's errors were not piped")?;
    let stopped = select! {
        done = timeout(LIMIT, watch(&mut child, stdout, stderr, on_code)) => match done {
            Ok(done) => return done,
            Err(_) => "The code was not entered within 15 minutes",
        },
        _ = cancelled => "Sign-in cancelled",
    };

    if let Err(e) = child.kill().await {
        log::warn!("stopping gh: {e}");
    }

    Err(refused(stopped))
}

/// Reads gh's output for the code and page, then waits for it to exit.
async fn watch(
    child: &mut Child,
    stdout: ChildStdout,
    stderr: ChildStderr,
    on_code: &Channel<DeviceCode>,
) -> Result<()> {
    let mut out = BufReader::new(stdout).lines();
    let mut err = BufReader::new(stderr).lines();
    let (mut out_open, mut err_open) = (true, true);
    let mut prompt = Prompt::default();
    let mut last = String::new();

    while out_open || err_open {
        let (line, from_out) = select! {
            line = out.next_line(), if out_open => (line, true),
            line = err.next_line(), if err_open => (line, false),
        };
        let Some(line) = line.context("reading gh's output")? else {
            if from_out {
                out_open = false;
            } else {
                err_open = false;
            }
            continue;
        };

        if let Some(device) = prompt.read(&line) {
            if let Err(e) = on_code.send(device.clone()) {
                log::warn!("showing the GitHub code: {e}");
            }

            if let Err(e) = crate::platform::open(&device.url) {
                log::warn!("opening GitHub's device page: {e}");
            }
        }

        if !line.trim().is_empty() {
            last = line;
        }
    }

    let status = child.wait().await.context("waiting for gh")?;
    if status.success() {
        Ok(())
    } else {
        Err(refused(&failure(&last)))
    }
}

/// What gh has printed of its sign-in prompt so far.
#[derive(Default)]
struct Prompt {
    code: Option<String>,
    url: Option<Url>,
    sent: bool,
}

impl Prompt {
    /// Reads one line; gives the code and page once both are known.
    fn read(&mut self, line: &str) -> Option<DeviceCode> {
        if let Some(code) = parse_code(line) {
            self.code = Some(code.to_owned());
        }
        if let Some(url) = parse_device_url(line) {
            self.url = Some(url);
        }
        if self.sent {
            return None;
        }

        let (Some(code), Some(url)) = (&self.code, &self.url) else {
            return None;
        };
        self.sent = true;
        Some(DeviceCode {
            code: code.clone(),
            url: url.to_string(),
        })
    }
}

/// The code in "! First copy your one-time code: AB23-77BA".
fn parse_code(line: &str) -> Option<&str> {
    const MARK: &str = "one-time code:";
    let at = line.to_ascii_lowercase().find(MARK)?;
    let code = line.get(at + MARK.len()..)?.split_whitespace().next()?;
    let valid = (4..=16).contains(&code.len())
        && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then_some(code)
}

/// GitHub's page in "Open this URL to continue in your web browser:
/// https://github.com/login/device". Only github.com pages open.
fn parse_device_url(line: &str) -> Option<Url> {
    let at = line.find("https://")?;
    let word = line[at..].split_whitespace().next()?;
    let word = word.trim_end_matches(['.', ',', ')', '"', '\'']);
    let url = Url::parse(word).ok()?;
    (url.scheme() == "https" && url.host_str() == Some("github.com")).then_some(url)
}

/// Why gh did not sign in, from the last line it printed.
fn failure(last: &str) -> String {
    let line = last.trim().trim_start_matches(['!', 'X']).trim();

    if line.is_empty() {
        return "gh could not sign in to GitHub".into();
    }

    let short: String = line.chars().take(REASON_CHARS).collect();
    format!("gh could not sign in to GitHub: {short}")
}

#[cfg(test)]
#[path = "../tests/settings_gh_sign_in.rs"]
mod tests;
