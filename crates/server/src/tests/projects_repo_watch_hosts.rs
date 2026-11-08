use super::*;
use beep_projects::model::{ForgejoHost, ForgejoUrl, Roles, Visibility};
use secrecy::SecretString;

fn project(roles: Roles) -> ProjectRow {
    ProjectRow {
        id: crate::projects::model::ProjectId(uuid::Uuid::from_u128(1)),
        name: "acme/widget".parse::<RepoName>().expect("a name"),
        roles,
        visibility: Visibility::Public,
        default_branch: "main".to_owned(),
        pin: None,
    }
}

fn opened(slot: Slot) -> Opened {
    Opened {
        slot,
        secret: SecretString::from("tok"),
        github: None,
    }
}

fn forge() -> IssueHost {
    IssueHost::Forgejo(
        ("forge.example.com")
            .parse::<ForgejoHost>()
            .expect("a host"),
    )
}

#[test]
fn a_mirror_on_github_and_forgejo_gives_both_primary_first() {
    let project = project(Roles {
        issues: Some(IssueHost::Github),
        prs: Some(IssueHost::Github),
    });
    let tokens = [
        opened(Slot::GitHub),
        opened(Slot::Forgejo(
            "https://forge.example.com"
                .parse::<ForgejoUrl>()
                .expect("a url"),
        )),
    ];
    let remotes = [
        "git@github.com:acme/widget.git".to_owned(),
        "ssh://git@forge.example.com:2222/mirrors/widget.git".to_owned(),
        "https://gitlab.example.org/acme/widget.git".to_owned(),
    ];
    let found = collect(
        &Sources {
            primary: Some(IssueHost::Github),
            project: &project,
            import_urls: Vec::new(),
            remotes: &remotes,
        },
        &Tokens(&tokens),
    );
    let got: Vec<(String, String, bool)> = found
        .iter()
        .map(|at| (at.name(), at.repo.to_string(), at.primary))
        .collect();
    assert_eq!(
        got,
        [
            ("github.com".to_owned(), "acme/widget".to_owned(), true),
            (
                "forge.example.com".to_owned(),
                "mirrors/widget".to_owned(),
                false
            ),
        ]
    );
}

#[test]
fn a_remote_without_a_token_or_of_another_repository_is_left_out() {
    let project = project(Roles::default());
    let tokens = [opened(Slot::GitHub)];
    let remotes = [
        "https://forge.example.com/acme/widget".to_owned(),
        "https://github.com/someone/other.git".to_owned(),
    ];
    let found = collect(
        &Sources {
            primary: Some(forge()),
            project: &project,
            import_urls: Vec::new(),
            remotes: &remotes,
        },
        &Tokens(&tokens),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].host, forge());
    assert!(found[0].primary);
}
