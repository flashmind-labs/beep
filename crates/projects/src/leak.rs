//! Keeps private details such as machine names, home paths and addresses out of published text.

/// Private details that must not reach published text: machine and user names, home paths, IPs and `host:port`.
#[derive(Debug, Clone)]
pub struct PublicText {
    /// Machine and user names, lowercase, as they were given.
    names: Vec<String>,
    home_paths: Vec<String>,
}

impl PublicText {
    /// The user name is the last segment of each of `home_paths`.
    pub fn new(machine_names: &[String], home_paths: &[String]) -> PublicText {
        let users = home_paths
            .iter()
            .filter_map(|path| path.trim_end_matches('/').rsplit('/').next())
            .map(str::to_owned);
        let mut names: Vec<String> = Vec::new();
        for name in machine_names.iter().cloned().chain(users) {
            let name = name.trim().to_owned();
            // Short names and generic accounts would hit ordinary words;
            // home paths still catch them.
            let generic = matches!(name.to_lowercase().as_str(), "root" | "user");
            if name.chars().count() >= 3 && !generic && !names.contains(&name) {
                names.push(name);
            }
        }
        let home_paths = home_paths
            .iter()
            .map(|path| path.trim_end_matches('/').to_owned())
            .filter(|path| !path.is_empty())
            .collect();
        PublicText { names, home_paths }
    }

    /// Also treats `paths` as private, without taking user names from them.
    pub fn with_paths(mut self, paths: &[String]) -> PublicText {
        for path in paths {
            let path = path.trim_end_matches('/');
            if !path.is_empty() && !self.home_paths.iter().any(|known| known == path) {
                self.home_paths.push(path.to_owned());
            }
        }
        self
    }

    /// The private details in `text`, each once: the name or literal found.
    pub fn hits(&self, text: &str) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        let mut add = |hit: &str| {
            if !found.iter().any(|seen| seen == hit) {
                found.push(hit.to_owned());
            }
        };
        let lower = text.to_lowercase();
        for name in &self.names {
            if has_whole(&lower, &name.to_lowercase()) {
                add(name);
            }
        }
        for path in &self.home_paths {
            if has_path(&lower, &path.to_lowercase()) {
                add(path);
            }
        }
        for raw in raw_tokens(text) {
            let v6 = trim_colon_edges(raw.trim_end_matches(['.', '-']));
            if is_address(v6) {
                add(v6);
                continue;
            }
            let token = raw.trim_matches(['.', ':', '-']);
            if is_address(token) || is_host_port(token) {
                add(token);
            }
        }
        found
    }
}

/// Whether `needle` is in `haystack` with no letter or digit on either side.
fn has_whole(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    haystack.match_indices(needle).any(|(at, _)| {
        let before = haystack[..at].chars().next_back();
        let after = haystack[at + needle.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// Whether `path` is in `haystack` not followed by a letter or digit.
fn has_path(haystack: &str, path: &str) -> bool {
    !path.is_empty()
        && haystack.match_indices(path).any(|(at, _)| {
            !haystack[at + path.len()..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric)
        })
}

/// The runs of address-like characters in `text`, edges untrimmed.
fn raw_tokens(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '[' | ']' | '-')))
        .filter(|token| !token.is_empty())
}

/// Drops one sentence colon at either edge, keeping `::`.
fn trim_colon_edges(token: &str) -> &str {
    let token = match token.strip_prefix(':') {
        Some(rest) if !rest.starts_with(':') => rest,
        _ => token,
    };
    match token.strip_suffix(':') {
        Some(rest) if !rest.ends_with(':') => rest,
        _ => token,
    }
}

fn is_ipv4(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            (1..=3).contains(&part.len())
                && part.bytes().all(|b| b.is_ascii_digit())
                && part.parse::<u16>().is_ok_and(|n| n <= 255)
        })
}

/// An IPv6 literal: `::` or eight groups, every group up to four hex digits.
fn is_ipv6(token: &str) -> bool {
    let mut parts: Vec<&str> = token.split(':').collect();
    // An embedded IPv4 tail, as in `::ffff:10.0.0.1`, stands for two groups.
    if parts.last().is_some_and(|last| is_ipv4(last)) {
        parts.pop();
        parts.extend(["0", "0"]);
    }
    let compressed = token.contains("::");
    parts.len() >= 3
        && (compressed || parts.len() == 8)
        && parts.iter().any(|part| !part.is_empty())
        && parts
            .iter()
            .all(|part| part.len() <= 4 && part.bytes().all(|b| b.is_ascii_hexdigit()))
}

fn is_address(token: &str) -> bool {
    let bare = token.trim_start_matches('[').trim_end_matches(']');
    is_ipv4(bare) || is_ipv6(bare)
}

fn is_port(text: &str) -> bool {
    (2..=5).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit())
}

/// A source or config file name, as in `leak.rs:120`.
fn is_file_name(host: &str) -> bool {
    const EXTENSIONS: [&str; 17] = [
        "rs", "ts", "tsx", "js", "py", "toml", "md", "json", "yml", "yaml", "sql", "kt", "swift",
        "go", "sh", "txt", "lock",
    ];
    host.rsplit_once('.')
        .is_some_and(|(_, ext)| EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}

/// The date and hour of an ISO time, as in `2026-11-01T12:30`.
fn is_timestamp(host: &str) -> bool {
    host.as_bytes()
        .windows(3)
        .any(|w| w[0].is_ascii_digit() && w[1] == b'T' && w[2].is_ascii_digit())
}

/// `name:port`, `1.2.3.4:port` or `[addr]:port`.
fn is_host_port(token: &str) -> bool {
    let Some((host, port)) = token.rsplit_once(':') else {
        return false;
    };
    if !is_port(port) || port.parse::<u32>().is_ok_and(|n| n > 65535) {
        return false;
    }
    if let Some(inner) = host.strip_prefix('[') {
        return inner.strip_suffix(']').is_some_and(is_address);
    }
    !host.is_empty()
        && !host.contains(':')
        && !is_file_name(host)
        && !is_timestamp(host)
        && (is_ipv4(host) || host.chars().any(char::is_alphabetic))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public() -> PublicText {
        PublicText::new(
            &["build-box".to_owned(), "nas-box".to_owned()],
            &["/home/sam".to_owned()],
        )
    }

    #[test]
    fn extra_paths_are_private_without_their_names() {
        let public = public().with_paths(&["/srv/widget".to_owned()]);
        assert_eq!(public.hits("built in /srv/widget/out"), ["/srv/widget"]);
        // The folder's name alone is the repository's, which is public.
        assert!(public.hits("the widget repo").is_empty());
    }

    #[test]
    fn public_text_hits() {
        let public = public();
        assert_eq!(public.hits("Built on build-box"), ["build-box"]);
        assert_eq!(public.hits("nas-box"), ["nas-box"]);
        assert!(
            public
                .hits("/home/sam/src")
                .contains(&"/home/sam".to_owned())
        );
        assert_eq!(public.hits("thanks sam."), ["sam"]);
        assert!(public.hits("a sample").is_empty());
        assert_eq!(public.hits("at 10.0.0.12 now"), ["10.0.0.12"]);
        assert_eq!(public.hits("on [fd00::1]:8080"), ["[fd00::1]:8080"]);
        assert_eq!(public.hits("fe80::1 and fe80::1"), ["fe80::1"]);
        assert_eq!(public.hits("db:5432"), ["db:5432"]);
        assert_eq!(public.hits("localhost:8080"), ["localhost:8080"]);
        assert!(public.hits("Fix the build box layout").is_empty());
        assert!(public.hits("version 1.2.3").is_empty());
        assert!(public.hits("localhost is fine").is_empty());
        assert!(public.hits("meet at 12:30:45 or 12:30").is_empty());
        assert!(public.hits("std::fmt and Vec::new").is_empty());
        for text in [
            "leak.rs:120",
            "Cargo.toml:14",
            "at 2026-11-01T12:30",
            "x:70000",
        ] {
            assert!(public.hits(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn ipv6_edges_are_kept() {
        let public = public();
        for literal in ["::1", "fe80::", "::ffff:10.0.0.1"] {
            assert_eq!(public.hits(&format!("see {literal}.")), [literal]);
        }
    }

    #[test]
    fn home_paths_end_at_the_name() {
        let public = public();
        assert!(public.hits("/home/samuel/src").is_empty());
        assert!(
            public
                .hits("/HOME/Sam/src")
                .contains(&"/home/sam".to_owned())
        );
    }

    #[test]
    fn generic_and_short_names_are_not_words() {
        let public = PublicText::new(
            &["pi".to_owned()],
            &["/root".to_owned(), "/home/user".to_owned()],
        );
        assert!(public.hits("the root user runs pi").is_empty());
        assert_eq!(public.hits("/home/user/src"), ["/home/user"]);
        assert_eq!(public.hits("cd /root"), ["/root"]);
    }
}
