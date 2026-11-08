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
