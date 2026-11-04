//! This Mac's own `gh` and `teacli` sign-ins, so Settings can save a git
//! host's token without the user pasting it. The token is read here and
//! goes straight to the server: the bar only learns whether there is one.

use crate::{
    beep::Beep,
    error::{DesktopError, Result},
    settings::credentials::{self, CredentialView, Kind},
};
use anyhow::Context;
use beep_proto::SecretText;
use serde::Serialize;
use std::{
    env,
    ffi::OsStr,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tauri::State;
use tokio::{fs, process::Command, time::timeout};
use url::Url;

/// How long `gh` may take to answer, on a slow network too.
const DEADLINE: Duration = Duration::from_secs(8);
/// Where Homebrew and installers put tools, for an app that did not get
/// the login shell's `PATH`.
const TOOL_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// A CLI's sign-in on this Mac.
enum Login {
    SignedIn {
        token: SecretText,
        account: Option<String>,
    },
    SignedOut,
    Missing,
}

/// A CLI's sign-in as the bar sees it: never the token.
#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LoginView {
    SignedIn { account: Option<String> },
    SignedOut,
    Missing,
}

impl From<&Login> for LoginView {
    fn from(login: &Login) -> Self {
        match login {
            Login::SignedIn { account, .. } => Self::SignedIn {
                account: account.clone(),
            },
            Login::SignedOut => Self::SignedOut,
            Login::Missing => Self::Missing,
        }
    }
}

/// A Forgejo server teacli on this Mac has a token for.
#[derive(Debug, Serialize)]
pub struct TeaServer {
    url: String,
}

/// One github.com account `gh` here is signed in to.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GhAccount {
    login: String,
    /// The one gh uses now.
    active: bool,
}

/// What Settings can take from this Mac's CLIs.
#[derive(Debug, Serialize)]
pub struct CliLogins {
    github: LoginView,
    /// Every github.com account gh has, from `gh auth status`; one at most
    /// with a gh from before several accounts.
    github_accounts: Vec<GhAccount>,
    forgejo: Vec<TeaServer>,
}

/// Whether `gh` is signed in here, to which accounts, and the teacli
/// servers with a token.
#[tauri::command]
pub async fn cli_logins() -> Result<CliLogins> {
    let github_accounts = match find_tool("gh") {
        Some(gh) => gh_accounts(&gh).await?,
        None => Vec::new(),
    };
    let github = match github_accounts.iter().find(|account| account.active) {
        Some(active) => LoginView::SignedIn {
            account: Some(active.login.clone()),
        },
        None => LoginView::from(&gh_login(true).await?),
    };
    let forgejo = tea_logins()
        .await?
        .into_iter()
        .map(|login| TeaServer { url: login.url })
        .collect();
    Ok(CliLogins {
        github,
        github_accounts,
        forgejo,
    })
}

/// Saves the token gh here holds for the github.com account `login`, as
/// one of the GitHub accounts, checked by the server.
#[tauri::command]
pub async fn use_gh_account(beep: State<'_, Arc<Beep>>, login: String) -> Result<CredentialView> {
    let login = parse_login(&login).ok_or_else(|| refused("That is not a GitHub login"))?;
    let gh = find_tool("gh").ok_or_else(|| refused("gh is not installed on this Mac"))?;
    let token = gh_account_token(&gh, &login).await?;
    credentials::save(&beep, Kind::Github, token, true).await
}

/// The github.com accounts gh lists, signed in or not checked.
async fn gh_accounts(gh: &Path) -> Result<Vec<GhAccount>> {
    // It exits with an error when any account's token is bad, and still
    // lists the rest.
    let text = all_output(gh, &["auth", "status", "--hostname", "github.com"]).await?;
    Ok(parse_gh_status(&text))
}

/// The token gh holds for `login`. A gh from before several accounts has
/// no `--user`; its one account's token is the active one.
async fn gh_account_token(gh: &Path, login: &str) -> Result<SecretText> {
    let args = ["auth", "token", "--hostname", "github.com", "--user", login];
    if let Some(token) = stdout(gh, &args).await?.as_deref().and_then(parse_token) {
        return Ok(token);
    }
    let active = gh_accounts(gh)
        .await?
        .into_iter()
        .any(|account| account.active && account.login.eq_ignore_ascii_case(login));
    if active {
        return gh_token().await;
    }
    Err(refused(&format!(
        "gh has no token for @{login} on this Mac"
    )))
}

/// The github.com accounts in what `gh auth status --hostname github.com`
/// printed. gh 2.40 and later list each account with its "Active account"
/// line; older gh prints one "Logged in to github.com as <login>", which is
/// the active one. An account gh failed to sign in is left out.
fn parse_gh_status(text: &str) -> Vec<GhAccount> {
    let mut accounts: Vec<GhAccount> = Vec::new();
    // Whether the lines now are about the last account pushed.
    let mut current = false;
    for line in text.lines().map(str::trim) {
        if line.contains("Failed to log in") {
            current = false;
        } else if let Some(rest) = line.split_once("Logged in to github.com account ") {
            current = push_login(&mut accounts, rest.1, false);
        } else if let Some(rest) = line.split_once("Logged in to github.com as ") {
            current = push_login(&mut accounts, rest.1, true);
        } else if let Some(value) = line.strip_prefix("- Active account:")
            && current
            && let Some(last) = accounts.last_mut()
        {
            last.active = value.trim() == "true";
        }
    }
    accounts
}

/// Pushes the login `rest` starts with, such as `octo-one (keyring)`;
/// whether it was one.
fn push_login(accounts: &mut Vec<GhAccount>, rest: &str, active: bool) -> bool {
    let word = rest.split_whitespace().next().unwrap_or_default();
    match parse_login(word) {
        Some(login)
            if !accounts
                .iter()
                .any(|a| a.login.eq_ignore_ascii_case(&login)) =>
        {
            accounts.push(GhAccount { login, active });
            true
        }
        _ => false,
    }
}

/// Saves the token `gh` or teacli holds for `host`, checked by the server
/// as a pasted one is.
#[tauri::command]
pub async fn use_cli_login(beep: State<'_, Arc<Beep>>, host: Kind) -> Result<CredentialView> {
    let token = match &host {
        Kind::Github => gh_token().await?,
        Kind::Forgejo { url } => tea_logins()
            .await?
            .into_iter()
            .find(|login| same_server(&login.url, url))
            .map(|login| login.token)
            .ok_or_else(|| refused("teacli has no token for this server"))?,
        Kind::ApiKey { .. } => return Err(refused("API keys have no CLI sign-in")),
    };
    credentials::save(&beep, host, token, true).await
}

/// Saves the token `gh` holds for github.com, checked by the server.
pub(crate) async fn save_gh_login(beep: &Beep) -> Result<CredentialView> {
    credentials::save(beep, Kind::Github, gh_token().await?, true).await
}

/// The token `gh` holds for github.com, or why there is none.
async fn gh_token() -> Result<SecretText> {
    match gh_login(false).await? {
        Login::SignedIn { token, .. } => Ok(token),
        Login::SignedOut => Err(refused("gh is not signed in on this Mac")),
        Login::Missing => Err(refused("gh is not installed on this Mac")),
    }
}

pub(crate) fn refused(reason: &str) -> DesktopError {
    DesktopError::BadRequest(reason.into())
}

/// `gh`'s github.com sign-in; with `account`, also the login it is for.
async fn gh_login(account: bool) -> Result<Login> {
    let Some(gh) = find_tool("gh") else {
        return Ok(Login::Missing);
    };
    let Some(out) = stdout(&gh, &["auth", "token", "--hostname", "github.com"]).await? else {
        return Ok(Login::SignedOut);
    };
    let Some(token) = parse_token(&out) else {
        return Ok(Login::SignedOut);
    };
    let account = if account {
        // Offline the sign-in still counts; the server finds the login.
        match stdout(&gh, &["api", "user", "--jq", ".login"]).await {
            Ok(out) => out.as_deref().and_then(parse_login),
            Err(e) => {
                log::warn!("asking gh for its account: {e}");
                None
            }
        }
    } else {
        None
    };
    Ok(Login::SignedIn { token, account })
}

/// What `program` prints when it succeeds, or `None` when it exits with
/// an error.
async fn stdout(program: &Path, args: &[&str]) -> Result<Option<String>> {
    let run = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = timeout(DEADLINE, run)
        .await
        .with_context(|| format!("{} took too long", program.display()))?
        .with_context(|| format!("running {}", program.display()))?;
    if !out.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8(out.stdout)
        .with_context(|| format!("{} printed something other than text", program.display()))?;
    Ok(Some(text))
}

/// What `program` prints, stdout then stderr, whether it succeeds or not.
async fn all_output(program: &Path, args: &[&str]) -> Result<String> {
    let run = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = timeout(DEADLINE, run)
        .await
        .with_context(|| format!("{} took too long", program.display()))?
        .with_context(|| format!("running {}", program.display()))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(text)
}

/// The token `gh auth token` printed, when it printed one.
fn parse_token(out: &str) -> Option<SecretText> {
    let token = out.trim();
    (!token.is_empty() && !token.contains(char::is_whitespace)).then(|| SecretText::new(token))
}

/// A GitHub login, when `out` is one.
fn parse_login(out: &str) -> Option<String> {
    let login = out.trim();
    let valid = !login.is_empty()
        && login.len() <= 39
        && login.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    valid.then(|| login.to_owned())
}

/// The first `name` on `PATH`, then in the usual tool folders.
pub(crate) fn find_tool(name: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH").unwrap_or_default();
    let home = env::var_os("HOME").map(PathBuf::from);
    tool_dirs(&path, home.as_deref())
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|file| file.is_file())
}

/// Where to look for a tool: `PATH` first, then Homebrew's folders and
/// `~/.local/bin`, which an app opened from Finder may not have.
fn tool_dirs(path: &OsStr, home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = env::split_paths(path)
        .filter(|dir| dir.is_absolute())
        .collect();
    let extra = TOOL_DIRS
        .iter()
        .map(PathBuf::from)
        .chain(home.map(|home| home.join(".local/bin")));
    for dir in extra {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

/// One server in teacli's config, with its token.
struct TeaLogin {
    url: String,
    token: SecretText,
}

/// The servers teacli's config has a token for. teacli has no command
/// that prints a token, so this reads `teacli.ini` as teacli does.
async fn tea_logins() -> Result<Vec<TeaLogin>> {
    let Some(file) = tea_config() else {
        return Ok(Vec::new());
    };
    match fs::read_to_string(&file).await {
        Ok(text) => Ok(parse_tea(&text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(anyhow::Error::new(e)
            .context(format!("reading {}", file.display()))
            .into()),
    }
}

/// `teacli.ini` under `$XDG_CONFIG_HOME`, or else `~/.config`.
fn tea_config() -> Option<PathBuf> {
    let config = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("teacli").join("teacli.ini"))
}

/// Each `[server]` section with both a web `url` and a `token`.
fn parse_tea(text: &str) -> Vec<TeaLogin> {
    #[derive(Default)]
    struct Section {
        url: Option<String>,
        token: Option<String>,
    }
    fn finish(section: Section, logins: &mut Vec<TeaLogin>) {
        if let (Some(url), Some(token)) = (section.url, section.token)
            && Url::parse(&url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
            && !token.is_empty()
        {
            logins.push(TeaLogin {
                url,
                token: SecretText::new(token),
            });
        }
    }
    let mut logins = Vec::new();
    let mut section = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(done) = section.take() {
                finish(done, &mut logins);
            }
            let name = line[1..line.len() - 1].trim();
            section = (name != "global").then(Section::default);
            continue;
        }
        let (Some(current), Some((key, value))) = (section.as_mut(), line.split_once('=')) else {
            continue;
        };
        let value = value.trim().to_owned();
        match key.trim() {
            "url" => current.url = Some(value),
            "token" => current.token = Some(value),
            _ => {}
        }
    }
    if let Some(done) = section {
        finish(done, &mut logins);
    }
    logins
}

/// Whether two server URLs name the same server: scheme, host and port.
fn same_server(a: &str, b: &str) -> bool {
    match (Url::parse(a.trim()), Url::parse(b.trim())) {
        (Ok(a), Ok(b)) => a.origin() == b.origin() && a.host_str().is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_printed_token_is_read_and_nothing_else_is() {
        let token = parse_token("gho_abc123\n").expect("one token");
        assert_eq!(token.expose(), "gho_abc123");
        assert!(parse_token("  \n").is_none());
        assert!(parse_token("no oauth token found for github.com").is_none());
    }

    #[test]
    fn a_login_is_what_github_allows() {
        assert_eq!(parse_login("you-work\n").as_deref(), Some("you-work"));
        assert_eq!(parse_login(""), None);
        assert_eq!(parse_login("{\"message\":\"Bad credentials\"}"), None);
    }

    const STATUS_TWO: &str = "github.com
  \u{2713} Logged in to github.com account octo-one (keyring)
  - Active account: false
  - Git operations protocol: https
  - Token: gho_************************************
  - Token scopes: 'gist', 'read:org', 'repo', 'workflow'

  \u{2713} Logged in to github.com account octo-two (keyring)
  - Active account: true
  - Git operations protocol: https
  - Token: gho_************************************
  - Token scopes: 'gist', 'read:org', 'repo'

  X Failed to log in to github.com account octo-three (keyring)
  - Active account: false
  - The token in keyring is invalid.
";

    #[test]
    fn gh_status_lists_each_account_and_the_active_one() {
        assert_eq!(
            parse_gh_status(STATUS_TWO),
            [
                GhAccount {
                    login: "octo-one".into(),
                    active: false
                },
                GhAccount {
                    login: "octo-two".into(),
                    active: true
                },
            ]
        );
    }

    #[test]
    fn gh_status_with_one_account() {
        let one = "github.com\n  \u{2713} Logged in to github.com account octo-one (keyring)\n  \
                   - Active account: true\n  - Git operations protocol: https\n";
        assert_eq!(
            parse_gh_status(one),
            [GhAccount {
                login: "octo-one".into(),
                active: true
            }]
        );
        // A gh from before several accounts.
        let old = "github.com\n  \u{2713} Logged in to github.com as octo-one \
                   (/srv/home/.config/gh/hosts.yml)\n  \u{2713} Git operations for github.com \
                   configured to use https protocol.\n  \u{2713} Token: gho_*****\n";
        assert_eq!(
            parse_gh_status(old),
            [GhAccount {
                login: "octo-one".into(),
                active: true
            }]
        );
        assert!(
            parse_gh_status(
                "You are not logged into any GitHub hosts. To log in, run: gh auth login\n"
            )
            .is_empty()
        );
    }

    #[test]
    fn tools_are_looked_for_past_a_bare_path() {
        let dirs = tool_dirs(
            OsStr::new("/usr/bin:/bin:relative"),
            Some(Path::new("/home/me")),
        );
        let dirs: Vec<_> = dirs
            .iter()
            .map(|d| d.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            dirs,
            [
                "/usr/bin",
                "/bin",
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/home/me/.local/bin"
            ]
        );
        let once = tool_dirs(OsStr::new("/opt/homebrew/bin"), None);
        assert_eq!(once.len(), 2);
    }

    #[test]
    fn teacli_servers_with_a_token_are_read() {
        let ini = "# managed elsewhere\n[global]\ndefault_server = work\n\n\
                   [work]\nurl   = https://git.example.com\ntoken = abc123\n\n\
                   [half]\nurl = https://code.example.org\n\n\
                   [bad]\nurl = not a url\ntoken = x\n\n\
                   [last]\n; comment\ntoken=def456\nurl=http://git.example.net:8087\n";
        let logins = parse_tea(ini);
        let found: Vec<_> = logins
            .iter()
            .map(|l| (l.url.as_str(), l.token.expose()))
            .collect();
        assert_eq!(
            found,
            [
                ("https://git.example.com", "abc123"),
                ("http://git.example.net:8087", "def456")
            ]
        );
    }

    #[test]
    fn servers_match_by_scheme_host_and_port() {
        assert!(same_server(
            "https://git.example.com",
            "https://git.example.com/"
        ));
        assert!(same_server(
            "https://Git.Example.com:443",
            "https://git.example.com"
        ));
        assert!(!same_server(
            "http://git.example.com",
            "https://git.example.com"
        ));
        assert!(!same_server(
            "https://git.example.com:3000",
            "https://git.example.com"
        ));
        assert!(!same_server("git.example.com", "git.example.com"));
    }

    #[test]
    fn the_bar_never_gets_the_token() {
        let login = Login::SignedIn {
            token: SecretText::new("gho_secret"),
            account: Some("you".into()),
        };
        let json = serde_json::to_string(&LoginView::from(&login)).expect("serializes");
        assert_eq!(json, r#"{"state":"signed_in","account":"you"}"#);
        assert!(!json.contains("gho_secret"));
    }
}
