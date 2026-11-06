//! Signing `gh` in to GitHub from Settings: Beep runs `gh auth login --web`,
//! shows the one-time code it prints and opens GitHub's device page. Once
//! the code is entered there gh exits, and its token is saved as "Use gh
//! sign-in" saves it.

use crate::{
    bar::app::open_url,
    beep::Beep,
    error::Result,
    fleet::projects::FleetState,
    settings::cli_login::{find_tool, refused, save_gh_login},
    settings::credentials::CredentialView,
};
use anyhow::Context;
use serde::Serialize;
use std::{
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};
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
    next_id: u64,
    running: Option<Running>,
}

/// Stops one sign-in, and tells when it has stopped.
struct Running {
    id: u64,
    cancel: oneshot::Sender<()>,
    ended: oneshot::Receiver<()>,
}

/// What a started sign-in holds: Cancel's signal, and a sender it drops
/// once gh is gone.
struct Started {
    id: u64,
    cancelled: oneshot::Receiver<()>,
    ended: oneshot::Sender<()>,
}

impl GhSignIn {
    fn lock(&self) -> MutexGuard<'_, Slot> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn start(&self) -> Result<Started> {
        let mut slot = self.lock();
        if slot.running.is_some() {
            return Err(refused("A GitHub sign-in is already under way"));
        }
        slot.next_id += 1;
        let id = slot.next_id;
        let (cancel, cancelled) = oneshot::channel();
        let (ended_tx, ended) = oneshot::channel();
        slot.running = Some(Running { id, cancel, ended });
        Ok(Started {
            id,
            cancelled,
            ended: ended_tx,
        })
    }

    /// Forgets sign-in `id` once it is over, unless another took its place.
    fn finish(&self, id: u64) {
        let mut slot = self.lock();
        if slot.running.as_ref().is_some_and(|r| r.id == id) {
            slot.running = None;
        }
    }

    /// Stops the sign-in under way, which kills gh.
    fn stop(&self) -> Option<oneshot::Receiver<()>> {
        let running = self.lock().running.take()?;
        // An error means the sign-in had just ended on its own.
        if running.cancel.send(()).is_err() {
            log::debug!("the GitHub sign-in had already ended");
        }
        Some(running.ended)
    }

    /// Stops the sign-in under way as Beep quits. The future ends once gh
    /// is killed, so it does not outlive the app; quitting bounds the wait.
    pub fn stop_on_quit(&self) -> impl Future<Output = ()> + Send + 'static {
        let ended = self.stop();
        async move {
            // The sender is dropped, never sent on, once gh is gone.
            if let Some(ended) = ended {
                ended.await.ok();
            }
        }
    }
}

/// Clears the slot however the sign-in ends.
struct Finish<'a> {
    sign_in: &'a GhSignIn,
    id: u64,
}

impl Drop for Finish<'_> {
    fn drop(&mut self) {
        self.sign_in.finish(self.id);
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
    beep: State<'_, Arc<Beep>>,
    sign_in: State<'_, GhSignIn>,
    on_code: Channel<DeviceCode>,
) -> Result<CredentialView> {
    let gh = find_tool("gh").ok_or_else(|| refused("gh is not installed on this Mac"))?;
    let Started {
        id,
        cancelled,
        ended,
    } = sign_in.start()?;
    let _finish = Finish {
        sign_in: &sign_in,
        id,
    };
    run_gh(&gh, &on_code, cancelled).await?;
    // Dropped only now, after gh is gone, so quitting waits for that.
    drop(ended);
    save_gh_login(&fleet, &beep).await
}

/// Stops the GitHub sign-in under way.
#[tauri::command]
pub fn cancel_gh_sign_in(sign_in: State<'_, GhSignIn>) {
    sign_in.stop();
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
            if let Err(e) = open_url(device.url) {
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
mod tests {
    use super::*;

    #[test]
    fn the_code_and_page_are_read_from_gh() {
        let mut prompt = Prompt::default();
        assert_eq!(
            prompt.read("! First copy your one-time code: AB23-77BA"),
            None
        );
        assert_eq!(
            prompt.read(
                "Open this URL to continue in your web browser: https://github.com/login/device"
            ),
            Some(DeviceCode {
                code: "AB23-77BA".into(),
                url: "https://github.com/login/device".into(),
            })
        );
        // Once only, however gh repeats itself.
        assert_eq!(
            prompt.read("! First copy your one-time code: AB23-77BA"),
            None
        );
    }

    #[test]
    fn the_page_may_come_first() {
        let mut prompt = Prompt::default();
        assert_eq!(
            prompt.read("Press Enter to open https://github.com/login/device in your browser..."),
            None
        );
        let device = prompt
            .read("! First copy your one-time code: 1F2E-3D4C")
            .expect("both known");
        assert_eq!(device.code, "1F2E-3D4C");
        assert_eq!(device.url, "https://github.com/login/device");
    }

    #[test]
    fn only_a_code_is_taken_as_one() {
        assert_eq!(
            parse_code("! First copy your One-Time Code: AB23-77BA"),
            Some("AB23-77BA")
        );
        assert_eq!(parse_code("! First copy your one-time code:"), None);
        assert_eq!(parse_code("one-time code: <script>"), None);
        assert_eq!(parse_code("Authentication complete."), None);
    }

    #[test]
    fn only_github_pages_open() {
        assert_eq!(
            parse_device_url("Open this URL: https://github.com/login/device.")
                .map(|u| u.to_string()),
            Some("https://github.com/login/device".into())
        );
        assert_eq!(
            parse_device_url("Open https://example.com/login/device"),
            None
        );
        assert_eq!(
            parse_device_url("Open http://github.com/login/device"),
            None
        );
        assert_eq!(parse_device_url("no link here"), None);
    }

    #[test]
    fn a_failure_says_what_gh_said() {
        assert_eq!(
            failure("X Failed to authenticate: expired_token\n"),
            "gh could not sign in to GitHub: Failed to authenticate: expired_token"
        );
        assert_eq!(failure("  "), "gh could not sign in to GitHub");
        assert!(failure(&"x".repeat(500)).len() < 260);
    }

    #[test]
    fn one_sign_in_at_a_time() {
        let sign_in = GhSignIn::default();
        let first = sign_in.start().expect("nothing under way");
        assert!(sign_in.start().is_err());
        // An old sign-in ending does not clear a newer one.
        let ended = sign_in.stop().expect("one under way");
        let second = sign_in.start().expect("the first was stopped");
        sign_in.finish(first.id);
        assert!(sign_in.start().is_err());
        sign_in.finish(second.id);
        assert!(sign_in.start().is_ok());
        drop(ended);
    }
}
