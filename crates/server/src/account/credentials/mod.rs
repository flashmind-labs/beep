//! Credentials a user adds once, kept sealed and handed to their machines (`Down::Credentials`).

mod check;
#[cfg(test)]
mod machines;
mod net;
mod seal;

pub use check::host_client;
pub use seal::{CredentialKey, HarnessOwner, Owner, WebhookOwner};

use crate::protocol::{
    SecretText,
    node::{EnvSecret, ForgejoLogin, MachineCredentials},
};

use beep_projects::{
    credentials::{Opened, Slot, github_for},
    github_owner_var,
    model::ForgejoUrl,
};
use secrecy::ExposeSecret;

/// Whether every address `url`'s host resolves to is public.
pub async fn is_public(url: &ForgejoUrl) -> bool {
    net::resolves_public(url.url()).await
}

/// What the machines get: `GH_TOKEN` for the default, plus a [`github_owner_var`] per owner on another account.
pub fn for_machines(opened: Vec<Opened>) -> MachineCredentials {
    let mut out = MachineCredentials::default();
    let mut owners: Vec<&str> = Vec::new();
    for login in opened.iter().filter_map(|o| o.github.as_ref()) {
        for owner in std::iter::once(&login.login).chain(&login.orgs) {
            if !owners.iter().any(|seen| seen.eq_ignore_ascii_case(owner)) {
                owners.push(owner);
            }
        }
    }
    for owner in owners {
        let Some(picked) = github_for(&opened, owner) else {
            continue;
        };
        let is_default = picked.github.as_ref().is_some_and(|login| login.default);
        if let (false, Some(name)) = (is_default, github_owner_var(owner)) {
            out.env.push(EnvSecret {
                name,
                value: SecretText::new(picked.secret.expose_secret()),
            });
        }
    }
    for Opened {
        slot,
        secret,
        github,
    } in opened
    {
        let value = SecretText::new(secret.expose_secret());
        match slot {
            Slot::Forgejo(url) => out.forgejo.push(ForgejoLogin {
                url: url.as_str().to_owned(),
                token: value,
            }),
            // Only the default account is GH_TOKEN.
            Slot::GitHub if !github.is_some_and(|login| login.default) => {}
            other => {
                if let Some(name) = other.env_var() {
                    out.env.push(EnvSecret {
                        name: name.to_owned(),
                        value,
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use beep_projects::credentials::GitHubLogin;
    use secrecy::SecretString;

    fn account(login: &str, orgs: &[&str], default: bool, token: &str) -> Opened {
        Opened {
            slot: Slot::GitHub,
            secret: SecretString::from(token),
            github: Some(GitHubLogin {
                login: login.into(),
                orgs: orgs.iter().map(|org| (*org).to_owned()).collect(),
                default,
            }),
        }
    }

    #[test]
    fn machines_get_the_default_as_gh_token_and_others_by_owner() {
        let opened = vec![
            account("octo-one", &["acme"], true, "tok-one"),
            account("octo-two", &["widget-co", "acme"], false, "tok-two"),
        ];
        let out = for_machines(opened);
        let env: Vec<(&str, &str)> = out
            .env
            .iter()
            .map(|var| (var.name.as_str(), var.value.expose()))
            .collect();
        assert_eq!(
            env,
            [
                ("BEEP_GH_TOKEN_OCTO_TWO", "tok-two"),
                ("BEEP_GH_TOKEN_WIDGET_CO", "tok-two"),
                ("GH_TOKEN", "tok-one"),
            ]
        );
    }
}
