//! Which repository the user means: `org/repo`, a GitHub URL, or a URL on a saved Forgejo server.

use super::ImportError;
use crate::{
    account::credentials::{ForgejoUrl, Opened, Slot, github_for},
    projects::model::{RepoName, is_github, remote_parts},
};
use secrecy::ExposeSecret;
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
    /// The repository `input` names; a non-GitHub URL must be on one of `forgejo`.
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
            let base = Url::parse(server.as_str()).ok()?;
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

    /// The user's non-empty token for this repository's host (on GitHub, the owner's account).
    pub(crate) fn credential<'a>(&self, credentials: &'a [Opened]) -> Option<&'a Opened> {
        match &self.host {
            Host::GitHub => github_for(credentials, &self.org),
            Host::Forgejo(url) => credentials
                .iter()
                .find(|opened| matches!(&opened.slot, Slot::Forgejo(saved) if saved == url)),
        }
        .filter(|opened| !opened.secret.expose_secret().is_empty())
    }

    pub(super) fn new(host: Host, org: &str, name: &str) -> Result<Self, ImportError> {
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
    pub(crate) fn sibling(&self, name: &RepoName) -> Result<Self, ImportError> {
        Self::new(self.host.clone(), name.owner(), name.repo())
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

/// A remote's host and path, lowercased without `.git` (`github.com/org/repo`); `None` for non-URLs.
pub(crate) fn remote_key(remote: &str) -> Option<String> {
    let (host, path) = remote_parts(remote.trim())?;
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    (!host.is_empty() && !path.is_empty() && !host.contains('/'))
        .then(|| format!("{host}/{path}").to_ascii_lowercase())
}

/// Whether `origin` is the repository at `url`, over https or ssh, ignoring case, `.git` and server sub-paths.
pub(crate) fn same_repo(url: &str, origin: &str, servers: &[ForgejoUrl]) -> bool {
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
pub(super) fn valid_part(part: &str) -> bool {
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
        // Another port on a saved server's host is another server.
        assert_eq!(
            parse("https://git.example.com:8443/team/app"),
            Err(ImportError::UnknownHost("git.example.com".into()))
        );
        assert!(parse("ssh://git@git.example.com:2222/team/app.git").is_ok());
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
    fn an_imported_url_matches_its_checkouts_in_any_form() {
        let url = "https://github.com/acme/widget";
        for origin in [
            "https://github.com/acme/widget",
            "https://github.com/acme/widget.git",
            "https://GitHub.com/Acme/Widget.git/",
            "git@github.com:acme/widget.git",
            "git@github.com:ACME/widget",
            "ssh://git@github.com/acme/widget.git",
            "ssh://git@github.com:22/acme/widget",
            "https://user@github.com/acme/widget",
        ] {
            assert!(same_repo(url, origin, &forge()), "{origin}");
        }
        for origin in [
            "https://github.com/acme/widget-docs",
            "https://github.com/other/widget",
            "https://git.example.com/acme/widget",
            "/srv/mirrors/acme/widget.git",
            "",
        ] {
            assert!(!same_repo(url, origin, &forge()), "{origin}");
        }
        // A Forgejo server under a sub-path, reached over ssh from the root.
        let sub = "https://code.example.org/forge/team/app";
        assert!(same_repo(
            sub,
            "git@code.example.org:team/app.git",
            &forge()
        ));
        assert!(same_repo(
            sub,
            "https://code.example.org/forge/team/app.git",
            &[]
        ));
        assert!(!same_repo(
            sub,
            "git@code.example.org:team/other.git",
            &forge()
        ));
    }
}
