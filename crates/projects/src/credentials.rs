//! The git credentials a user saved, opened: which token reads which host.

use beep_proto::Service;
use secrecy::SecretString;

/// The last four characters of `secret`, never more than a quarter of one shorter than 16.
pub fn last4(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    let shown = if chars.len() >= 16 {
        4
    } else {
        chars.len() / 4
    };
    chars[chars.len() - shown..].iter().collect()
}

/// One credential opened for the machines.
pub struct Opened {
    pub service: Service,
    pub secret: SecretString,
    /// Which account a GitHub token is; `None` for every other service.
    pub github: Option<GitHubLogin>,
}

/// Who a saved GitHub token signs in as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubLogin {
    pub login: String,
    /// The orgs the token could see when it was saved.
    pub orgs: Vec<String>,
    /// The account machines get as `GH_TOKEN`.
    pub default: bool,
}

impl GitHubLogin {
    /// Whether `owner` is this account or one of its orgs.
    fn owns(&self, owner: &str) -> bool {
        self.login.eq_ignore_ascii_case(owner)
            || self.orgs.iter().any(|org| org.eq_ignore_ascii_case(owner))
    }
}

/// The GitHub token for `owner`'s repos: that account, else one with `owner` among its orgs, else the default.
pub fn github_for<'a>(opened: &'a [Opened], owner: &str) -> Option<&'a Opened> {
    let accounts = || {
        opened
            .iter()
            .filter_map(|o| o.github.as_ref().map(|login| (o, login)))
    };
    accounts()
        .find(|(_, login)| login.login.eq_ignore_ascii_case(owner))
        .or_else(|| accounts().find(|(_, login)| login.owns(owner)))
        .or_else(|| accounts().find(|(_, login)| login.default))
        .map(|(opened, _)| opened)
}

#[cfg(test)]
#[path = "tests/credentials.rs"]
mod tests;
