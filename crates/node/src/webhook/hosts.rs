//! What a watch asks of the repository's host, through the CLI signed in
//! on this machine: `gh` for GitHub, `teacli` for Forgejo and Gitea. Each
//! deletes only a webhook it is given the id of.

use crate::provision::spawn_env::{self, Scope};
use beep_proto::{GITHUB_ENV_VAR, node::webhook::PollReason};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};
use std::{collections::BTreeSet, future::Future, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStderr, Command},
    time::timeout,
};

/// How long one host command may take.
const COMMAND_LIMIT: Duration = Duration::from_secs(30);
/// How long `gh webhook forward` may take to connect.
const FORWARD_START: Duration = Duration::from_secs(30);
/// The most of a tool's output kept for the log.
const MAX_DETAIL: usize = 400;

/// GitHub's events the project page shows the effects of.
pub(super) const GITHUB_EVENTS: &str = "push,pull_request,pull_request_review,issues,\
    issue_comment,check_run,check_suite,workflow_run,workflow_job";
/// Forgejo's and Gitea's. A server ignores the names it does not know,
/// such as the Actions events of an older one.
const FORGEJO_EVENTS: [&str; 13] = [
    "push",
    "pull_request",
    "pull_request_sync",
    "pull_request_comment",
    "pull_request_review_approved",
    "pull_request_review_rejected",
    "issues",
    "issue_comment",
    "workflow_run",
    "workflow_job",
    "action_run_failure",
    "action_run_success",
    "action_run_recover",
];

/// Why the host gives no live updates, with what the tool said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Refusal {
    pub(super) why: PollReason,
    pub(super) detail: String,
}

impl Refusal {
    pub(super) fn new(why: PollReason, detail: impl Into<String>) -> Self {
        Self {
            why,
            detail: tail(&detail.into()),
        }
    }
}

/// A webhook made, and the forwarder that keeps it going for GitHub.
pub(super) struct Made {
    /// `None` when its id could not be found; it is then the forwarder's
    /// to end.
    pub(super) hook: Option<u64>,
    pub(super) forward: Option<Forward>,
}

/// A running `gh webhook forward`.
pub(super) struct Forward {
    pub(super) child: Child,
    pub(super) stderr: Lines<BufReader<ChildStderr>>,
}

/// What a watch asks of a host.
pub(super) trait HostApi: Send + Sync + 'static {
    /// Whether the signed-in account is an admin of the repository.
    fn is_admin(&self) -> impl Future<Output = Result<bool, Refusal>> + Send;
    /// Makes a webhook that posts to `url`, signed with `secret`.
    fn make(
        &self,
        url: &str,
        secret: &SecretString,
    ) -> impl Future<Output = Result<Made, Refusal>> + Send;
    /// Deletes webhook `id`; `Ok` when it is gone, deleted now or before.
    fn delete(&self, id: u64) -> impl Future<Output = Result<(), Refusal>> + Send;
}

/// The end of `text`, the part that says why.
fn tail(text: &str) -> String {
    let text = text.trim();
    let mut start = text.len().saturating_sub(MAX_DETAIL);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

/// What a finished command gave.
struct Ran {
    ok: bool,
    out: String,
}

/// Runs `program` with the user's credentials, `extra` env vars and
/// `input` on stdin.
async fn run(
    program: &str,
    args: &[&str],
    extra: &[(&str, String)],
    input: Option<&[u8]>,
) -> Result<Ran, Refusal> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    spawn_env::apply(&mut command, Scope::Shell);
    for (name, value) in extra {
        command.env(name, value);
    }
    let mut child = command
        .spawn()
        .map_err(|e| Refusal::new(PollReason::NoTool, format!("{program}: {e}")))?;
    if let Some(input) = input
        && let Some(mut stdin) = child.stdin.take()
    {
        stdin
            .write_all(input)
            .await
            .map_err(|e| Refusal::new(PollReason::Failed, e.to_string()))?;
    }
    let output = timeout(COMMAND_LIMIT, child.wait_with_output())
        .await
        .map_err(|_| Refusal::new(PollReason::Failed, format!("{program} took too long")))?
        .map_err(|e| Refusal::new(PollReason::Failed, e.to_string()))?;
    let mut out = String::from_utf8_lossy(&output.stdout).into_owned();
    out.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(Ran {
        ok: output.status.success(),
        out,
    })
}

/// Whether a failed delete says the webhook is not there.
fn gone(out: &str) -> bool {
    let out = out.to_ascii_lowercase();
    out.contains("404") || out.contains("not found")
}

/// `permissions.admin` of a repository as the host's API gives it.
fn admin_of(out: &str) -> Result<bool, Refusal> {
    let repo: Value = serde_json::from_str(out.trim())
        .map_err(|_| Refusal::new(PollReason::Failed, "the repository did not read"))?;
    Ok(repo["permissions"]["admin"].as_bool().unwrap_or(false))
}

/// GitHub through `gh`.
pub(super) struct GitHub {
    pub(super) repo: String,
    /// `GH_TOKEN` for this repository's owner, when it is not the default.
    pub(super) token: Option<String>,
}

impl GitHub {
    /// The token for `repo` from the credentials the node holds: the
    /// owner's own var when there is one, else the default.
    pub(super) fn new(repo: &str, token_var: Option<&str>) -> Self {
        let token = token_var.and_then(|wanted| {
            spawn_env::vars(Scope::Shell)
                .into_iter()
                .find(|(name, _)| name.to_str() == Some(wanted))
                .and_then(|(_, value)| value.into_string().ok())
        });
        Self {
            repo: repo.to_owned(),
            token,
        }
    }

    fn env(&self) -> Vec<(&'static str, String)> {
        self.token
            .iter()
            .map(|token| (GITHUB_ENV_VAR, token.clone()))
            .collect()
    }

    async fn gh(&self, args: &[&str]) -> Result<Ran, Refusal> {
        run("gh", args, &self.env(), None).await
    }

    /// The ids of the repository's `gh webhook forward` webhooks.
    async fn forward_hooks(&self) -> Result<BTreeSet<u64>, Refusal> {
        let path = format!("repos/{}/hooks", self.repo);
        let ran = self
            .gh(&["api", &path, "--jq", ".[] | select(.name == \"cli\") | .id"])
            .await?;
        if !ran.ok {
            return Err(Refusal::new(PollReason::Failed, ran.out));
        }
        Ok(ran
            .out
            .lines()
            .filter_map(|line| line.trim().parse().ok())
            .collect())
    }

    /// Installs gh's webhook extension when it is missing.
    async fn ensure_extension(&self) -> Result<(), Refusal> {
        if self.gh(&["webhook", "--help"]).await?.ok {
            return Ok(());
        }
        log::info!("installing gh's webhook extension");
        let ran = self.gh(&["extension", "install", "cli/gh-webhook"]).await?;
        if ran.ok {
            Ok(())
        } else {
            Err(Refusal::new(PollReason::NoTool, ran.out))
        }
    }
}

impl HostApi for GitHub {
    async fn is_admin(&self) -> Result<bool, Refusal> {
        let path = format!("repos/{}", self.repo);
        let ran = self.gh(&["api", &path]).await?;
        if !ran.ok {
            return Err(Refusal::new(PollReason::NoTool, ran.out));
        }
        admin_of(&ran.out)
    }

    async fn make(&self, url: &str, secret: &SecretString) -> Result<Made, Refusal> {
        self.ensure_extension().await?;
        let before = self.forward_hooks().await?;
        let mut command = Command::new("gh");
        // The secret is an argument: gh takes it no other way.
        command
            .args([
                "webhook",
                "forward",
                "--repo",
                &self.repo,
                "--events",
                GITHUB_EVENTS,
                "--url",
                url,
                "--secret",
                secret.expose_secret(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        spawn_env::apply(&mut command, Scope::Shell);
        for (name, value) in self.env() {
            command.env(name, value);
        }
        let mut child = command
            .spawn()
            .map_err(|e| Refusal::new(PollReason::NoTool, format!("gh: {e}")))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| Refusal::new(PollReason::Failed, "gh has no stderr"))?;
        let mut stderr = BufReader::new(stderr).lines();
        let mut said = String::new();
        let started = timeout(FORWARD_START, async {
            while let Ok(Some(line)) = stderr.next_line().await {
                if line.contains("Forwarding Webhook events") {
                    return true;
                }
                said.push_str(&line);
                said.push('\n');
            }
            false
        })
        .await;
        if started != Ok(true) {
            // GitHub allows one forward per repository: another one
            // running says "Hook already exists".
            return Err(Refusal::new(PollReason::Failed, said));
        }
        let after = self.forward_hooks().await;
        let hook = after
            .ok()
            .and_then(|after| after.difference(&before).max().copied());
        Ok(Made {
            hook,
            forward: Some(Forward { child, stderr }),
        })
    }

    async fn delete(&self, id: u64) -> Result<(), Refusal> {
        let path = format!("repos/{}/hooks/{id}", self.repo);
        let ran = self.gh(&["api", "-X", "DELETE", &path]).await?;
        if ran.ok || gone(&ran.out) {
            Ok(())
        } else {
            Err(Refusal::new(PollReason::Failed, ran.out))
        }
    }
}

/// Forgejo or Gitea through `teacli`.
pub(super) struct Forgejo {
    pub(super) repo: String,
    /// The server as teacli knows it.
    pub(super) host: String,
}

impl Forgejo {
    async fn teacli(&self, args: &[&str], input: Option<&[u8]>) -> Result<Ran, Refusal> {
        let mut all = vec!["api"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--host", &self.host]);
        run("teacli", &all, &[], input).await
    }
}

/// The body that makes a webhook to `url`.
fn forgejo_hook(url: &str, secret: &SecretString) -> Value {
    json!({
        "type": "gitea",
        "active": true,
        "events": FORGEJO_EVENTS,
        "config": {
            "url": url,
            "content_type": "json",
            "secret": secret.expose_secret(),
        },
    })
}

impl HostApi for Forgejo {
    async fn is_admin(&self) -> Result<bool, Refusal> {
        let path = format!("repos/{}", self.repo);
        let ran = self.teacli(&[&path], None).await?;
        if !ran.ok {
            return Err(Refusal::new(PollReason::NoTool, ran.out));
        }
        admin_of(&ran.out)
    }

    async fn make(&self, url: &str, secret: &SecretString) -> Result<Made, Refusal> {
        let path = format!("repos/{}/hooks", self.repo);
        // On stdin, so the secret is in no process's arguments.
        let body = forgejo_hook(url, secret).to_string();
        let ran = self
            .teacli(
                &[&path, "-X", "POST", "--input", "-"],
                Some(body.as_bytes()),
            )
            .await?;
        if !ran.ok {
            return Err(Refusal::new(PollReason::Failed, ran.out));
        }
        let made: Value = serde_json::from_str(ran.out.trim())
            .map_err(|_| Refusal::new(PollReason::Failed, "the new webhook did not read"))?;
        let hook = made["id"]
            .as_u64()
            .ok_or_else(|| Refusal::new(PollReason::Failed, "the new webhook has no id"))?;
        Ok(Made {
            hook: Some(hook),
            forward: None,
        })
    }

    async fn delete(&self, id: u64) -> Result<(), Refusal> {
        let path = format!("repos/{}/hooks/{id}", self.repo);
        let ran = self.teacli(&[&path, "-X", "DELETE"], None).await?;
        if ran.ok || gone(&ran.out) {
            Ok(())
        } else {
            Err(Refusal::new(PollReason::Failed, ran.out))
        }
    }
}
