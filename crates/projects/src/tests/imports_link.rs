use super::*;
use crate::model::RepoName;
use std::str::FromStr;

fn folder(remotes: &[(&str, &str)]) -> FolderRemotes {
    FolderRemotes {
        folder: "/srv/widget".into(),
        remotes: remotes
            .iter()
            .map(|(name, url)| ((*name).to_owned(), (*url).to_owned()))
            .collect(),
    }
}

fn repos() -> (RepoRef, RepoRef) {
    let repo = RepoRef::parse("acme/widget", &[]).expect("a repo");
    let fork = repo
        .sibling(&RepoName::from_str("octo/widget").expect("a name"))
        .expect("a fork");
    (repo, fork)
}

#[test]
fn a_checkout_of_the_fork_gets_upstream_unless_the_name_is_taken() {
    let (repo, fork) = repos();
    let plan = plan(
        &folder(&[("origin", "https://github.com/octo/widget")]),
        &repo,
        &fork,
    )
    .expect("a plan");
    assert_eq!(plan.fork_remote, "origin");
    assert_eq!(plan.upstream_remote, "upstream");
    assert_eq!(plan.add[0].url, "https://github.com/acme/widget.git");
    let both = plan_of(&[
        ("origin", "https://github.com/octo/widget"),
        ("up", "https://github.com/acme/widget"),
    ]);
    assert!(both.add.is_empty());
    assert_eq!(both.upstream_remote, "up");
    let (repo, fork) = repos();
    assert_eq!(
        super::plan(
            &folder(&[
                ("origin", "https://github.com/octo/widget"),
                ("upstream", "https://github.com/someone/widget"),
            ]),
            &repo,
            &fork
        ),
        None
    );
}

fn plan_of(remotes: &[(&str, &str)]) -> Plan {
    let (repo, fork) = repos();
    plan(&folder(remotes), &repo, &fork).expect("a plan")
}
