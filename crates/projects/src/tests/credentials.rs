use super::*;
use secrecy::ExposeSecret;

fn account(login: &str, orgs: &[&str], default: bool, token: &str) -> Opened {
    Opened {
        service: Service::GitHub,
        secret: SecretString::from(token),
        github: Some(GitHubLogin {
            login: login.into(),
            orgs: orgs.iter().map(|org| (*org).to_owned()).collect(),
            default,
        }),
    }
}

fn token(opened: Option<&Opened>) -> &str {
    opened.map_or("", |o| o.secret.expose_secret())
}

#[test]
fn a_repository_owner_picks_its_github_account() {
    let opened = [
        account("octo-one", &["acme", "shared"], true, "tok-one"),
        account("octo-two", &["widget-co", "shared"], false, "tok-two"),
        account("octo-three", &["widget-co"], false, "tok-three"),
    ];
    assert_eq!(token(github_for(&opened, "octo-two")), "tok-two");
    assert_eq!(token(github_for(&opened, "OCTO-THREE")), "tok-three");
    assert_eq!(token(github_for(&opened, "Widget-Co")), "tok-two");
    assert_eq!(token(github_for(&opened, "acme")), "tok-one");
    assert_eq!(token(github_for(&opened, "shared")), "tok-one");
    assert_eq!(token(github_for(&opened, "someone-else")), "tok-one");
    assert!(github_for(&[], "acme").is_none());
}

#[test]
fn last4_shows_little_of_short_secrets() {
    assert_eq!(last4("ghp_abcdefghijkl4f2a"), "4f2a");
    assert_eq!(last4("short123"), "23");
    assert_eq!(last4(""), "");
}
