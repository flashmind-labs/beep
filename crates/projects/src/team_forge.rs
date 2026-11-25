//! The forge calls a team needs: who may be assigned, assigning, and the signed-in login. The
//! builders and readers are pure, for the node and the desktop; the async functions run them on
//! a machine the project is on.

use crate::{
    commands::{GH_ISSUES, GH_OPEN_LIMIT, GH_PRS, gh_account, json, pages, parse_login, text},
    issues::{IssueSource, SourceError, host_failed, issue_source, run_rendered},
    issues::{Machines, ProjectStore},
    model::{IssueHost, RepoName},
    quote,
};
use anyhow::Result;
use beep_proto::team::ForgeLogin;
use serde_json::Value;
use url::form_urlencoded::byte_serialize;

/// A forge CLI invocation: the program and its arguments, none quoted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForgeCall {
    pub program: &'static str,
    pub args: Vec<String>,
}

impl ForgeCall {
    /// `gh api` or `teacli api` of `path`, then `rest`.
    fn api(host: &IssueHost, path: String, rest: &[&str]) -> ForgeCall {
        let (program, mut args) = match host {
            IssueHost::Github => ("gh", vec!["api".to_owned(), path]),
            IssueHost::Forgejo(host) => (
                "teacli",
                vec![
                    "api".to_owned(),
                    path,
                    "--host".to_owned(),
                    host.as_ref().to_owned(),
                ],
            ),
        };
        args.extend(rest.iter().map(|arg| (*arg).to_owned()));

        ForgeCall { program, args }
    }

    /// `gh` with `words`.
    fn gh<'a>(words: impl IntoIterator<Item = &'a str>) -> ForgeCall {
        ForgeCall {
            program: "gh",
            args: words.into_iter().map(str::to_owned).collect(),
        }
    }

    /// The shell command for `repo`'s owner, quoted; `gh` runs as the user's account there.
    pub fn render(&self, repo: &RepoName) -> String {
        let words: Vec<String> = std::iter::once(self.program)
            .chain(self.args.iter().map(String::as_str))
            .map(quote)
            .collect();
        let account = match self.program {
            "gh" => gh_account(&repo.to_string()),
            _ => String::new(),
        };

        format!("{account}{}", words.join(" "))
    }
}

/// `GET repos/{repo}/collaborators/{login}/permission`: what `login` may do there.
pub fn permission_call(host: &IssueHost, repo: &RepoName, login: &ForgeLogin) -> ForgeCall {
    let path = format!("repos/{repo}/collaborators/{}/permission", escaped(login));
    ForgeCall::api(host, path, &[])
}

/// Reads [`permission_call`]'s outcome: whether they may push, or `None` when the forge does
/// not say. Forgejo tells only a repository's admins what others may do and refuses the rest
/// with a 403; [`collaborator_call`] then says only who is no collaborator. GitHub reports
/// maintain as `write` in `permission` and names it in `role_name`; Forgejo also has `owner`.
pub fn permission_answer(
    exit_ok: bool,
    stdout: &str,
    stderr: &str,
) -> Result<Option<bool>, SourceError> {
    if !exit_ok {
        return match status_of(stderr) {
            Some("404") => Ok(Some(false)),
            Some("403") => Ok(None),
            _ => Err(SourceError::Refused(format!(
                "Could not check the collaborator: {}",
                stderr.trim()
            ))),
        };
    }

    let reply = json(stdout).map_err(|error| host_failed(&error))?;
    let pushes = ["permission", "role_name"].into_iter().any(|key| {
        matches!(
            &*text(&reply, key),
            "admin" | "owner" | "maintain" | "write"
        )
    });

    Ok(Some(pushes))
}

/// `GET repos/{repo}/collaborators/{login}`: 204 for a collaborator at any level, 404 when not.
pub fn collaborator_call(host: &IssueHost, repo: &RepoName, login: &ForgeLogin) -> ForgeCall {
    let path = format!("repos/{repo}/collaborators/{}", escaped(login));
    ForgeCall::api(host, path, &[])
}

/// Reads [`collaborator_call`]'s outcome as whether they may push: `false` for one who is no
/// collaborator. A collaborator may only read, so that is an error: it does not say yes. `gh
/// api` and `teacli api` both exit non-zero on a 404 and print its status ("HTTP 404") on
/// stderr, so a 404 token says no. A bare "not found" is also what a missing CLI prints, so it
/// is an error.
pub fn collaborator_answer(exit_ok: bool, stderr: &str) -> Result<bool, SourceError> {
    if exit_ok {
        return Err(SourceError::Refused(
            "Only the repository's admins see whether a collaborator may push".to_owned(),
        ));
    }
    if status_of(stderr) == Some("404") {
        return Ok(false);
    }

    Err(SourceError::Refused(format!(
        "Could not check the collaborator: {}",
        stderr.trim()
    )))
}

/// The HTTP status a failed `gh api` or `teacli api` printed: 403 or 404, as a token of its own.
fn status_of(stderr: &str) -> Option<&'static str> {
    let mut tokens = stderr.split(|c: char| !c.is_ascii_alphanumeric());

    tokens.find_map(|token| ["403", "404"].into_iter().find(|status| *status == token))
}

/// `GET repos/{repo}`: whether the signed-in account sees the repository.
pub fn repo_call(host: &IssueHost, repo: &RepoName) -> ForgeCall {
    ForgeCall::api(host, format!("repos/{repo}"), &[])
}

/// `POST repos/{repo}/statuses/{sha}`: marks commit `sha` with a status named `context`, in the
/// signed-in account's name. Only those who may push mark commits.
pub fn stamp_call(host: &IssueHost, repo: &RepoName, sha: &str, context: &str) -> ForgeCall {
    let context = format!("context={context}");
    let rest = ["-X", "POST", "-f", "state=success", "-f", &context];

    ForgeCall::api(host, format!("repos/{repo}/statuses/{sha}"), &rest)
}

/// `GET repos/{repo}/commits/{sha}/statuses`: the statuses on commit `sha`.
pub fn stamps_call(host: &IssueHost, repo: &RepoName, sha: &str) -> ForgeCall {
    ForgeCall::api(host, format!("repos/{repo}/commits/{sha}/statuses"), &[])
}

/// The logins that put a status whose context starts with `context` on the commit, as
/// [`stamps_call`] printed them. GitHub takes contexts in any case; teacli prints `null` for none.
pub fn parse_stampers(stdout: &str, context: &str) -> Result<Vec<String>, SourceError> {
    let statuses = json(stdout).map_err(|error| host_failed(&error))?;
    let statuses = match statuses {
        Value::Array(statuses) => statuses,
        Value::Null => Vec::new(),
        _ => return Err(host_failed(&anyhow::anyhow!("the statuses are not a list"))),
    };
    let logins = statuses
        .iter()
        .filter(|status| {
            let named = text(status, "context");
            named
                .get(..context.len())
                .is_some_and(|start| start.eq_ignore_ascii_case(context))
        })
        .filter_map(|status| status.get("creator"))
        .map(|creator| text(creator, "login"))
        .filter(|login| !login.is_empty())
        .collect();

    Ok(logins)
}

/// `GET user`, the signed-in account.
pub fn user_call(host: &IssueHost) -> ForgeCall {
    ForgeCall::api(host, "user".to_owned(), &[])
}

/// The login [`user_call`] printed.
pub fn parse_user_login(stdout: &str) -> Result<ForgeLogin, SourceError> {
    let login = parse_login(stdout).map_err(|error| host_failed(&error))?;

    login
        .parse()
        .map_err(|error| SourceError::Store(anyhow::Error::new(error)))
}

/// `GET repos/{repo}/issues/{number}`, to read the assignees Forgejo's PATCH replaces.
pub fn issue_call(host: &IssueHost, repo: &RepoName, number: u64) -> ForgeCall {
    ForgeCall::api(host, format!("repos/{repo}/issues/{number}"), &[])
}

/// The assignee logins in [`issue_call`]'s reply.
pub fn parse_assignees(stdout: &str) -> Result<Vec<String>, SourceError> {
    let issue = json(stdout).map_err(|error| host_failed(&error))?;
    let logins = match issue.get("assignees") {
        Some(Value::Array(users)) => users
            .iter()
            .map(|user| text(user, "login"))
            .filter(|login| !login.is_empty())
            .collect(),
        _ => Vec::new(),
    };

    Ok(logins)
}

/// The assignees after adding `new` to `current`, `current` first, no login twice.
pub fn merged_assignees(current: &[String], new: &[ForgeLogin]) -> Vec<String> {
    let mut all: Vec<String> = current.to_vec();

    for login in new {
        let login = login.to_string();
        if !all.iter().any(|have| have.eq_ignore_ascii_case(&login)) {
            all.push(login);
        }
    }

    all
}

/// Assigns `logins` to issue `number`. GitHub adds them (`POST .../assignees`); Forgejo's
/// `PATCH` replaces the whole list, so pass [`merged_assignees`] there.
pub fn assign_call(host: &IssueHost, repo: &RepoName, number: u64, logins: &[String]) -> ForgeCall {
    match host {
        IssueHost::Github => {
            let fields: Vec<String> = logins
                .iter()
                .flat_map(|login| ["-f".to_owned(), format!("assignees[]={login}")])
                .collect();
            let mut rest = vec!["-X", "POST"];
            rest.extend(fields.iter().map(String::as_str));

            ForgeCall::api(
                host,
                format!("repos/{repo}/issues/{number}/assignees"),
                &rest,
            )
        }
        IssueHost::Forgejo(_) => {
            let list = serde_json::to_string(logins).unwrap_or_else(|_| "[]".to_owned());
            let field = format!("assignees={list}");

            ForgeCall::api(
                host,
                format!("repos/{repo}/issues/{number}"),
                &["-X", "PATCH", "--raw-field", &field],
            )
        }
    }
}

/// The open issues, in the shape `parse_open_issues` reads: GitHub's as the desktop lists them,
/// up to `GH_OPEN_LIMIT` without pull requests, and every page of Forgejo's.
pub fn open_issues_call(host: &IssueHost, repo: &RepoName) -> ForgeCall {
    match host {
        IssueHost::Github => {
            let (repo, limit) = (repo.to_string(), GH_OPEN_LIMIT.to_string());
            let words = [
                "issue", "list", "-R", &repo, "--state", "open", "--limit", &limit,
            ];

            ForgeCall::gh(words.into_iter().chain(GH_ISSUES))
        }
        IssueHost::Forgejo(_) => ForgeCall::api(host, open_issues(host, repo), &["--paginate"]),
    }
}

/// The open issues assigned to `login`, every page:
/// `parse_open_issues` reads the pages `gh` prints one after another.
pub fn assigned_issues_call(host: &IssueHost, repo: &RepoName, login: &ForgeLogin) -> ForgeCall {
    let by = match host {
        IssueHost::Github => "assignee",
        IssueHost::Forgejo(_) => "assigned_by",
    };
    let path = format!("{}&{by}={}", open_issues(host, repo), escaped(login));

    ForgeCall::api(host, path, &["--paginate"])
}

/// The assignments and other events of issue `number`, every page: GitHub's
/// `events`, Forgejo's `timeline`.
pub fn assignment_events_call(host: &IssueHost, repo: &RepoName, number: u64) -> ForgeCall {
    let what = match host {
        IssueHost::Github => "events?per_page=100",
        IssueHost::Forgejo(_) => "timeline?limit=50",
    };

    ForgeCall::api(
        host,
        format!("repos/{repo}/issues/{number}/{what}"),
        &["--paginate"],
    )
}

/// Whether the latest assignment of `login` in [`assignment_events_call`]'s pages was made by
/// `login`: GitHub's `assigned` events name the `actor` and the `assignee`, Forgejo's `assignees`
/// entries the `user` and the `assignee`, with `removed_assignee` set on an unassignment. An
/// issue with no assignment of `login` in them is not self-assigned.
pub fn parse_self_assigned(stdout: &str, login: &ForgeLogin) -> Result<bool, SourceError> {
    let events = pages(stdout, "events").map_err(|error| host_failed(&error))?;
    let login = login.to_string();
    let named = |event: &Value, key: &str| {
        event
            .get(key)
            .map(|user| text(user, "login"))
            .filter(|who| !who.is_empty())
    };
    let latest = events
        .iter()
        .filter(|event| match &*text(event, "event") {
            "assigned" => true,
            _ => {
                &*text(event, "type") == "assignees"
                    && event.get("removed_assignee") != Some(&Value::Bool(true))
            }
        })
        .rfind(|event| {
            named(event, "assignee").is_some_and(|who| who.eq_ignore_ascii_case(&login))
        });

    Ok(latest.is_some_and(|event| {
        let by = named(event, "actor").or_else(|| named(event, "user"));

        by.is_some_and(|by| by.eq_ignore_ascii_case(&login))
    }))
}

/// The pull requests, open or closed, that may come from `branch`, in the shape `parse_open_prs`
/// reads: their head branches say whether `branch` has one. GitHub lists those from a branch of
/// that name in the repository or any fork; Forgejo cannot pick by branch, so it lists them all,
/// every page.
pub fn branch_pulls_call(host: &IssueHost, repo: &RepoName, branch: &str) -> ForgeCall {
    match host {
        IssueHost::Github => {
            let repo = repo.to_string();
            let words = [
                "pr", "list", "-R", &repo, "--head", branch, "--state", "all",
            ];

            ForgeCall::gh(words.into_iter().chain(["--limit", "100"]).chain(GH_PRS))
        }
        IssueHost::Forgejo(_) => {
            let path = format!("repos/{repo}/pulls?state=all&sort=recentupdate&limit=50");
            ForgeCall::api(host, path, &["--paginate"])
        }
    }
}

fn open_issues(host: &IssueHost, repo: &RepoName) -> String {
    match host {
        IssueHost::Github => format!("repos/{repo}/issues?state=open&per_page=100"),
        IssueHost::Forgejo(_) => format!("repos/{repo}/issues?state=open&type=issues&limit=50"),
    }
}

fn escaped(login: &ForgeLogin) -> String {
    byte_serialize(login.to_string().as_bytes()).collect()
}

/// Assigns `logins` to issue `number` through a machine the project is on. Forgejo has no call
/// that adds an assignee: its PATCH replaces the list, so an assignment made between the read
/// and the PATCH is lost. The list is read again after, and the logins that went missing are
/// added once more.
pub async fn assign(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
    number: u64,
    logins: &[ForgeLogin],
) -> Result<(), SourceError> {
    let source = source(store, machines).await?;
    let repo = store.name();
    let host = &source.host;

    if matches!(host, IssueHost::Github) {
        let call = assign_call(host, repo, number, &merged_assignees(&[], logins));
        run(machines, &source, repo, &call).await?;
        return Ok(());
    }

    let read = issue_call(host, repo, number);
    let mut current = parse_assignees(&run(machines, &source, repo, &read).await?)?;

    for _ in 0..2 {
        let call = assign_call(host, repo, number, &merged_assignees(&current, logins));
        run(machines, &source, repo, &call).await?;
        current = parse_assignees(&run(machines, &source, repo, &read).await?)?;

        if kept(&current, logins) {
            return Ok(());
        }
    }

    Err(SourceError::Refused(format!(
        "Another assignment of #{number} replaced this one; assign it again."
    )))
}

/// Whether every one of `logins` is among `assignees`.
fn kept(assignees: &[String], logins: &[ForgeLogin]) -> bool {
    merged_assignees(assignees, logins).len() == assignees.len()
}

/// The login the project's host machine is signed in as.
pub async fn my_login(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<ForgeLogin, SourceError> {
    let source = source(store, machines).await?;
    let out = run(machines, &source, store.name(), &user_call(&source.host)).await?;

    parse_user_login(&out)
}

async fn source(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
) -> Result<IssueSource, SourceError> {
    issue_source(store, machines).await?.ok_or_else(|| {
        SourceError::Refused("The project has no GitHub or Forgejo host.".to_owned())
    })
}

/// Runs `call` on `source`'s machine; a failed command is the [`crate::issues::HostExit`]
/// inside `Store`.
async fn run(
    machines: &dyn Machines,
    source: &IssueSource,
    repo: &RepoName,
    call: &ForgeCall,
) -> Result<String, SourceError> {
    let rendered = call.render(repo);
    let ran = match machines.gates() {
        Some(gates) => gates.run(source, &rendered, false).await,
        None => run_rendered(source.shell.as_ref(), &rendered).await,
    };

    ran.map_err(SourceError::Store)
}

#[cfg(test)]
#[path = "tests/team_forge.rs"]
mod tests;
