use super::*;
use std::str::FromStr;

#[test]
fn import_finds_checkout_by_fork_url() {
    let repo = RepoRef::parse("acme/widget", &[]).expect("a repo");
    let out = "@@dir /srv/fork/widget\n\
            remote.origin.url git@github.com:someone/widget.git\n\
            remote.upstream.url https://github.com/acme/widget\n\
            @@dir /srv/other\n\
            remote.origin.url https://github.com/acme/gadget.git\n\
            @@dir /srv/not-a-checkout\n\
            @@dir /srv/widget\n\
            remote.mine.url https://github.com/someone/widget\n\
            remote.origin.url https://github.com/Acme/Widget.git\n";
    let found = parse_remotes(out);
    assert_eq!(found.len(), 4);
    let matched: Vec<(&str, Option<String>)> = found
        .iter()
        .map(|folder| (folder.folder.as_str(), repo_remote(folder, &repo)))
        .collect();
    assert_eq!(
        matched,
        [
            ("/srv/fork/widget", Some("upstream".to_owned())),
            ("/srv/other", None),
            ("/srv/not-a-checkout", None),
            ("/srv/widget", Some("origin".to_owned())),
        ]
    );
}

#[test]
fn an_import_links_a_checkout_of_the_fork_or_the_repository() {
    let repo = RepoRef::parse("acme/widget", &[]).expect("a repo");
    let name = crate::model::RepoName::from_str("octo/widget").expect("a name");
    let fork = repo.sibling(&name).expect("a fork");
    let found = parse_remotes(
        "@@dir /srv/other\n\
             remote.origin.url https://github.com/acme/gadget.git\n\
             @@dir /srv/mirror\n\
             remote.mine.url https://github.com/acme/widget\n\
             remote.origin.url /srv/elsewhere\n\
             @@dir /srv/fork/widget\n\
             remote.origin.url git@github.com:octo/widget.git\n",
    );
    let picked = pick_existing(found.clone(), &repo, Some(&fork)).expect("a checkout");
    assert_eq!(picked.folder, "/srv/fork/widget");
    let without_fork = pick_existing(found.clone(), &repo, None).expect("a checkout");
    assert_eq!(without_fork.folder, "/srv/mirror");
    assert_eq!(pick_existing(found[..1].to_vec(), &repo, Some(&fork)), None);
}
