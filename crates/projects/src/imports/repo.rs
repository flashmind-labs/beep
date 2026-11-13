//! Which repository the user means: `org/repo`, a GitHub URL, or a URL on a saved Forgejo server.

use super::ImportError;
use crate::git_items::github::HOST as GITHUB;
use crate::model::{ForgejoUrl, MAX_REPO_PART, RepoName, is_github, remote_parts};
use url::Url;

/// Where a repository lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Host {
    GitHub,
    Forgejo(ForgejoUrl),
}

/// One repository on one host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub host: Host,
    org: String,
    name: String,
}

impl RepoRef {
    /// The repository `input` names; a non-GitHub URL must be on one of `forgejo`.
    pub fn parse(input: &str, forgejo: &[ForgejoUrl]) -> Result<Self, ImportError> {
        let input = input.trim();

        if input.is_empty() {
            return Err(ImportError::Malformed);
        }

        if let Some(rest) = input.strip_prefix("git@") {
            // scp-like: git@host:org/repo.git
            let (host, path) = rest.split_once(':').ok_or(ImportError::Malformed)?;
            return Self::on_host(host, None, path, forgejo);
        }

        if input.contains("://") {
            let url = Url::parse(input).map_err(|_| ImportError::Malformed)?;

            if !matches!(url.scheme(), "https" | "http" | "ssh" | "git") {
                return Err(ImportError::Malformed);
            }

            let host = url.host_str().ok_or(ImportError::Malformed)?;
            // An ssh URL's port is its own, never the web server's.
            let web_port = url.port().filter(|_| url.scheme().starts_with("http"));
            return Self::on_host(host, web_port, url.path(), forgejo);
        }

        let parts: Vec<&str> = input.trim_matches('/').split('/').collect();

        match parts[..] {
            [org, name] => Self::new(Host::GitHub, org, name),
            // `github.com/org/repo` without a scheme.
            [host, ..] if host.contains('.') => Self::on_host(
                host,
                None,
                input.trim_matches('/').split_at(host.len()).1,
                forgejo,
            ),
            _ => Err(ImportError::Malformed),
        }
    }

    /// The repository at `path` on `host`, with `web_port` when an http URL named one.
    fn on_host(
        host: &str,
        web_port: Option<u16>,
        path: &str,
        forgejo: &[ForgejoUrl],
    ) -> Result<Self, ImportError> {
        let host = host.to_ascii_lowercase();

        if is_github(&host) {
            let (org, name) = org_and_name(path)?;
            return Self::new(Host::GitHub, org, name);
        }

        let server = forgejo.iter().find_map(|server| {
            let base = Url::parse(server.as_ref()).ok()?;
            let same_host = base.host_str()? == host;
            let same_port = web_port.is_none() || base.port() == web_port;
            let prefix = base.path().trim_end_matches('/');
            let rest = path.strip_prefix(prefix)?;
            (same_host && same_port && (prefix.is_empty() || rest.starts_with('/')))
                .then(|| (server.clone(), rest.to_owned()))
        });
        // An ssh path has no sub-path: try it from the root too. A web
        // URL on another port is another server.
        let server = server.or_else(|| {
            forgejo
                .iter()
                .filter(|_| web_port.is_none())
                .find_map(|server| {
                    let base = Url::parse(server.as_ref()).ok()?;
                    (base.host_str()? == host).then(|| (server.clone(), path.to_owned()))
                })
        });
        let Some((server, rest)) = server else {
            return Err(ImportError::UnknownHost(host));
        };
        let (org, name) = org_and_name(&rest)?;
        Self::new(Host::Forgejo(server), org, name)
    }

    pub fn new(host: Host, org: &str, name: &str) -> Result<Self, ImportError> {
        let name = name.strip_suffix(".git").unwrap_or(name);

        if !valid_part(org) || !valid_part(name) {
            return Err(ImportError::Malformed);
        }

        Ok(Self {
            host,
            org: org.to_owned(),
            name: name.to_owned(),
        })
    }

    /// The repository `name` on the same host, such as the user's fork.
    pub fn sibling(&self, name: &RepoName) -> Result<Self, ImportError> {
        Self::new(self.host.clone(), name.owner(), name.repo())
    }

    /// `org/repo`, the project's name.
    pub fn project(&self) -> String {
        format!("{}/{}", self.org, self.name)
    }

    pub fn org(&self) -> &str {
        &self.org
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its web page, such as `https://github.com/org/repo`.
    pub fn web_url(&self) -> String {
        format!("{}/{}/{}", self.base(), self.org, self.name)
    }

    /// What git clones over https.
    pub fn clone_url(&self) -> String {
        format!("{}.git", self.web_url())
    }

    fn base(&self) -> String {
        match &self.host {
            Host::GitHub => format!("https://{GITHUB}"),
            Host::Forgejo(server) => server.as_ref().to_owned(),
        }
    }

    /// The host as the user knows it, such as "GitHub" or `git.example.com`.
    pub fn host_name(&self) -> String {
        match &self.host {
            Host::GitHub => "GitHub".to_owned(),
            Host::Forgejo(server) => server.host(),
        }
    }

    fn host_only(&self) -> String {
        match &self.host {
            Host::GitHub => GITHUB.to_owned(),
            Host::Forgejo(server) => Url::parse(server.as_ref())
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .unwrap_or_default(),
        }
    }

    /// Whether a checkout's `origin` URL, https or ssh, is this repository.
    pub fn matches_remote(&self, remote: &str) -> bool {
        let Some((host, path)) = remote_parts(remote.trim()) else {
            return false;
        };
        let path = path.trim_end_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        let tail = format!("/{}/{}", self.org, self.name).to_ascii_lowercase();
        host.eq_ignore_ascii_case(&self.host_only())
            && format!("/{}", path.trim_start_matches('/'))
                .to_ascii_lowercase()
                .ends_with(&tail)
    }
}

/// A remote's host and path, lowercased without `.git` (`github.com/org/repo`); `None` for non-URLs.
pub fn remote_key(remote: &str) -> Option<String> {
    let (host, path) = remote_parts(remote.trim())?;
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    (!host.is_empty() && !path.is_empty() && !host.contains('/'))
        .then(|| format!("{host}/{path}").to_ascii_lowercase())
}

/// Whether `origin` is the repository at `url`, over https or ssh, ignoring case, `.git` and server sub-paths.
pub fn same_repo(url: &str, origin: &str, servers: &[ForgejoUrl]) -> bool {
    let Some(key) = remote_key(origin) else {
        return false;
    };
    remote_key(url).is_some_and(|known| known == key)
        || RepoRef::parse(url, servers).is_ok_and(|repo| repo.matches_remote(origin))
}

/// The first two segments of `path`; the rest, such as `/tree/main`, is a page.
fn org_and_name(path: &str) -> Result<(&str, &str), ImportError> {
    let mut parts = path.split('/').filter(|part| !part.is_empty());

    match (parts.next(), parts.next()) {
        (Some(org), Some(name)) => Ok((org, name)),
        _ => Err(ImportError::Malformed),
    }
}

/// Letters, digits, `-`, `_` and `.`, not starting with a dot or dash: valid on hosts and as folders.
pub fn valid_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= MAX_REPO_PART
        && !part.starts_with(['.', '-'])
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Updates only URLs naming the renamed repository, preserving transport and credentials.
pub fn renamed_remote(
    remote: &str,
    rename: &beep_proto::node::mesh_state::RemoteRename,
) -> Option<String> {
    let from = Url::parse(&rename.from).ok()?;
    let to = Url::parse(&rename.to).ok()?;
    let (owner_path, old_name) = from.path().trim_end_matches('/').rsplit_once('/')?;
    let (new_owner_path, new_name) = to.path().trim_end_matches('/').rsplit_once('/')?;

    if from.origin() != to.origin() || owner_path != new_owner_path {
        return None;
    }

    let (host, path) = remote_parts(remote)?;
    if !from.host_str()?.eq_ignore_ascii_case(&host) {
        return None;
    }
    let path = path.trim_end_matches('/');
    let bare = path.strip_suffix(".git").unwrap_or(path);
    let ssh = !remote.contains("://") || remote.starts_with("ssh://");
    let full = format!("{owner_path}/{old_name}");
    let owner = owner_path.rsplit('/').next()?;
    let short = format!("{owner}/{old_name}");
    if !bare
        .trim_start_matches('/')
        .eq_ignore_ascii_case(full.trim_start_matches('/'))
        && !(ssh && bare.trim_start_matches('/').eq_ignore_ascii_case(&short))
    {
        return None;
    }

    if remote.contains("://") {
        let mut url = Url::parse(remote).ok()?;
        if !ssh
            && (url.scheme() != from.scheme()
                || url.port_or_known_default() != from.port_or_known_default())
        {
            return None;
        }
        let (prefix, _) = url.path().trim_end_matches('/').rsplit_once('/')?;
        let suffix = if path.ends_with(".git") { ".git" } else { "" };
        url.set_path(&format!("{prefix}/{new_name}{suffix}"));
        Some(url.to_string())
    } else {
        let (prefix, _) = remote.rsplit_once('/')?;
        let suffix = if path.ends_with(".git") { ".git" } else { "" };
        Some(format!("{prefix}/{new_name}{suffix}"))
    }
}

#[cfg(test)]
#[path = "../tests/imports_repo.rs"]
mod tests;
