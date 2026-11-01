//! Which repository the user means: `org/repo`, a GitHub URL, or a URL on a
//! Forgejo or Gitea server they saved a token for.

use super::ImportError;
use crate::credentials::ForgejoUrl;
use url::Url;

const GITHUB: &str = "github.com";
/// The longest org or repo name GitHub and Forgejo take.
const MAX_PART: usize = 100;

/// Where a repository lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Host {
    GitHub,
    Forgejo(ForgejoUrl),
}

/// One repository on one host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoRef {
    pub(crate) host: Host,
    org: String,
    name: String,
}

impl RepoRef {
    /// The repository `input` names. A URL on a host other than GitHub
    /// must be on one of `forgejo`, the servers the user has a token for.
    pub(crate) fn parse(input: &str, forgejo: &[ForgejoUrl]) -> Result<Self, ImportError> {
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
            return Self::on_host(host, url.port(), url.path(), forgejo);
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

    fn on_host(
        host: &str,
        port: Option<u16>,
        path: &str,
        forgejo: &[ForgejoUrl],
    ) -> Result<Self, ImportError> {
        let host = host.to_ascii_lowercase();
        if host == GITHUB || host == "www.github.com" {
            let (org, name) = org_and_name(path)?;
            return Self::new(Host::GitHub, org, name);
        }
        let server = forgejo.iter().find_map(|server| {
            let base = Url::parse(server.as_str()).ok()?;
            let same_host = base.host_str()? == host;
            // An ssh URL has its own port; an https one must match.
            let same_port = port.is_none() || base.port() == port;
            let prefix = base.path().trim_end_matches('/');
            let rest = path.strip_prefix(prefix)?;
            (same_host && same_port && (prefix.is_empty() || rest.starts_with('/')))
                .then(|| (server.clone(), rest.to_owned()))
        });
        // An ssh path has no sub-path: try it from the root too.
        let server = server.or_else(|| {
            forgejo.iter().find_map(|server| {
                let base = Url::parse(server.as_str()).ok()?;
                (base.host_str()? == host).then(|| (server.clone(), path.to_owned()))
            })
        });
        let Some((server, rest)) = server else {
            return Err(ImportError::UnknownHost(host));
        };
        let (org, name) = org_and_name(&rest)?;
        Self::new(Host::Forgejo(server), org, name)
    }

    fn new(host: Host, org: &str, name: &str) -> Result<Self, ImportError> {
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

    /// `org/repo`, the project's name.
    pub(crate) fn project(&self) -> String {
        format!("{}/{}", self.org, self.name)
    }

    pub(crate) fn org(&self) -> &str {
        &self.org
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Its web page, such as `https://github.com/org/repo`.
    pub(crate) fn web_url(&self) -> String {
        format!("{}/{}/{}", self.base(), self.org, self.name)
    }

    /// What git clones over https.
    pub(crate) fn clone_url(&self) -> String {
        format!("{}.git", self.web_url())
    }

    fn base(&self) -> String {
        match &self.host {
            Host::GitHub => format!("https://{GITHUB}"),
            Host::Forgejo(server) => server.as_str().to_owned(),
        }
    }

    /// The host as the user knows it, such as "GitHub" or `git.example.com`.
    pub(crate) fn host_name(&self) -> String {
        match &self.host {
            Host::GitHub => "GitHub".to_owned(),
            Host::Forgejo(server) => server.host(),
        }
    }

    fn host_only(&self) -> String {
        match &self.host {
            Host::GitHub => GITHUB.to_owned(),
            Host::Forgejo(server) => Url::parse(server.as_str())
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned))
                .unwrap_or_default(),
        }
    }

    /// Whether a checkout's `origin` URL, https or ssh, is this repository.
    pub(crate) fn matches_remote(&self, remote: &str) -> bool {
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

/// A remote URL's host, without port or user, and its path.
fn remote_parts(remote: &str) -> Option<(String, String)> {
    if remote.contains("://") {
        let url = Url::parse(remote).ok()?;
        return Some((url.host_str()?.to_owned(), url.path().to_owned()));
    }
    // scp-like: [user@]host:path
    let (host, path) = remote.split_once(':')?;
    let host = host.rsplit('@').next()?;
    Some((host.to_owned(), path.to_owned()))
}

/// The first two segments of `path`: the rest, such as `/tree/main`, is a
/// page of the repository.
fn org_and_name(path: &str) -> Result<(&str, &str), ImportError> {
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    match (parts.next(), parts.next()) {
        (Some(org), Some(name)) => Ok((org, name)),
        _ => Err(ImportError::Malformed),
    }
}

/// Letters, digits, `-`, `_` and `.`, not starting with a dot or a dash:
/// what both hosts allow, and safe as a folder name.
fn valid_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= MAX_PART
        && !part.starts_with(['.', '-'])
        && part
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forge() -> Vec<ForgejoUrl> {
        vec![
            "https://git.example.com".parse().unwrap(),
            "https://code.example.org/forge".parse().unwrap(),
        ]
    }

    fn parse(input: &str) -> Result<RepoRef, ImportError> {
        RepoRef::parse(input, &forge())
    }

    #[test]
    fn org_slash_repo_is_on_github() {
        let repo = parse(" n0-computer/iroh ").unwrap();
        assert_eq!(repo.host, Host::GitHub);
        assert_eq!(repo.project(), "n0-computer/iroh");
        assert_eq!(repo.web_url(), "https://github.com/n0-computer/iroh");
        assert_eq!(repo.clone_url(), "https://github.com/n0-computer/iroh.git");
        assert_eq!(repo.host_name(), "GitHub");
    }

    #[test]
    fn github_urls_in_any_common_form() {
        for input in [
            "https://github.com/n0-computer/iroh",
            "https://github.com/n0-computer/iroh.git",
            "https://github.com/n0-computer/iroh/",
            "https://github.com/n0-computer/iroh/tree/main/iroh",
            "https://www.github.com/n0-computer/iroh",
            "github.com/n0-computer/iroh",
            "git@github.com:n0-computer/iroh.git",
            "ssh://git@github.com/n0-computer/iroh.git",
        ] {
            let repo = parse(input).unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(repo.host, Host::GitHub, "{input}");
            assert_eq!(repo.project(), "n0-computer/iroh", "{input}");
        }
    }

    #[test]
    fn forgejo_urls_match_a_saved_server() {
        let repo = parse("https://git.example.com/team/app").unwrap();
        assert_eq!(
            repo.host,
            Host::Forgejo("https://git.example.com".parse().unwrap())
        );
        assert_eq!(repo.clone_url(), "https://git.example.com/team/app.git");
        assert_eq!(repo.host_name(), "git.example.com");
        let sub = parse("https://code.example.org/forge/team/app.git").unwrap();
        assert_eq!(sub.web_url(), "https://code.example.org/forge/team/app");
        assert_eq!(parse("git@git.example.com:team/app.git").unwrap(), repo);
    }

    #[test]
    fn unknown_hosts_and_bad_names_are_refused() {
        assert_eq!(
            parse("https://gitlab.com/team/app"),
            Err(ImportError::UnknownHost("gitlab.com".into()))
        );
        for bad in [
            "",
            "iroh",
            "a/b/c",
            "org/..",
            "org/-rm",
            "org/re po",
            "org/x'y",
            "https://github.com/onlyorg",
            "ftp://github.com/a/b",
        ] {
            assert_eq!(parse(bad), Err(ImportError::Malformed), "{bad}");
        }
    }

    #[test]
    fn remotes_match_by_host_and_path_in_https_or_ssh() {
        let repo = parse("n0-computer/iroh").unwrap();
        assert!(repo.matches_remote("https://github.com/n0-computer/iroh.git"));
        assert!(repo.matches_remote("https://github.com/N0-Computer/iroh"));
        assert!(repo.matches_remote("git@github.com:n0-computer/iroh.git"));
        assert!(repo.matches_remote("https://user@github.com/n0-computer/iroh"));
        assert!(!repo.matches_remote("https://github.com/n0-computer/iroh-gossip"));
        assert!(!repo.matches_remote("https://git.example.com/n0-computer/iroh"));
        assert!(!repo.matches_remote(""));
        let sub = parse("https://code.example.org/forge/team/app").unwrap();
        assert!(sub.matches_remote("https://code.example.org/forge/team/app.git"));
        assert!(sub.matches_remote("ssh://git@code.example.org:2222/team/app.git"));
    }
}
