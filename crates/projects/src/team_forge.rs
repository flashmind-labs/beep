//! The forge calls a team needs: who may be assigned, assigning, and the signed-in login. The
//! builders and readers are pure, for the node and the desktop; the async functions run them on
//! a machine the project is on.

use crate::{
    commands::{gh_account, json, parse_login, text},
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

/// The open issues, in the shape `parse_open_issues` reads. GitHub's list also holds pull
/// requests, which that parser drops.
pub fn open_issues_call(host: &IssueHost, repo: &RepoName) -> ForgeCall {
    ForgeCall::api(host, open_issues(host, repo), &[])
}

/// The open issues assigned to `login`, as [`open_issues_call`] lists them, every page:
/// `parse_open_issues` reads the pages `gh` prints one after another.
pub fn assigned_issues_call(host: &IssueHost, repo: &RepoName, login: &ForgeLogin) -> ForgeCall {
    let by = match host {
        IssueHost::Github => "assignee",
        IssueHost::Forgejo(_) => "assigned_by",
    };
    let path = format!("{}&{by}={}", open_issues(host, repo), escaped(login));

    ForgeCall::api(host, path, &["--paginate"])
}

/// The pull requests, open or closed, that may come from `branch`, every page, in the shape
/// `parse_open_prs` reads: their head branches say whether `branch` has one. GitHub lists those
/// of the repository's `branch`; Forgejo cannot pick by branch, so it lists them all.
pub fn branch_pulls_call(host: &IssueHost, repo: &RepoName, branch: &str) -> ForgeCall {
    let path = match host {
        IssueHost::Github => {
            let head: String =
                byte_serialize(format!("{}:{branch}", repo.owner()).as_bytes()).collect();
            format!("repos/{repo}/pulls?state=all&head={head}&per_page=100")
        }
        IssueHost::Forgejo(_) => format!("repos/{repo}/pulls?state=all&sort=recentupdate&limit=50"),
    };

    ForgeCall::api(host, path, &["--paginate"])
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

/// Assigns `logins` to issue `number` through a machine the project is on.
pub async fn assign(
    store: &dyn ProjectStore,
    machines: &dyn Machines,
    number: u64,
    logins: &[ForgeLogin],
) -> Result<(), SourceError> {
    let source = source(store, machines).await?;
    let repo = store.name();
    let names = match source.host {
        IssueHost::Github => merged_assignees(&[], logins),
        IssueHost::Forgejo(_) => {
            let current = issue_call(&source.host, repo, number);
            let out = run(machines, &source, repo, &current).await?;

            merged_assignees(&parse_assignees(&out)?, logins)
        }
    };
    run(
        machines,
        &source,
        repo,
        &assign_call(&source.host, repo, number, &names),
    )
    .await?;

    Ok(())
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
