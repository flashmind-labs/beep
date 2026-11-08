use super::*;
use crate::{api::project_sync::project_name, projects::model::memory_scope};
use beep_projects::imports::{
    place::{Folders, checkout_folder},
    repo::RepoRef,
};

/// Every rule a project name must pass across imports, sync, memory and checkout folders.
fn passes_every_rule(name: &str) {
    let repo = (name)
        .parse::<RepoName>()
        .unwrap_or_else(|e| panic!("{name}: {e}"));
    // A project's memories are scoped by its repository part.
    memory_scope(&repo).expect("a memory scope");
    assert_eq!(project_name(name).ok(), Some(name));
    let import = RepoRef::parse(name, &[]).unwrap_or_else(|e| panic!("{name}: {e:?}"));
    assert_eq!(import.project(), name);
    let folders = Folders {
        os: Some("linux"),
        home: Some("/home/user"),
        workspace: None,
    };
    let folder = checkout_folder(folders, &import).expect("a checkout folder");
    assert_eq!(folder, format!("/home/user/projects/{name}"));
}

#[test]
fn a_copy_of_a_repository_name_is_still_one() {
    assert_eq!(copy_name("acme/app", 1), "acme/app-copy");
    assert_eq!(copy_name("acme/app", 3), "acme/app-copy-3");
    passes_every_rule(&copy_name("acme/app", 1));
    passes_every_rule(&copy_name("Acme/My_App.rs", 12));
    // The old form fails the import's rules.
    assert!("acme/app (2)".parse::<RepoName>().is_err());
    assert!(RepoRef::parse("acme/app (2)", &[]).is_err());

    let long = format!("acme/{}", "r".repeat(MAX_REPO_PART));
    let copy = copy_name(&long, 1000);
    assert!(copy.ends_with("-copy-1000"));
    passes_every_rule(&copy);
}

#[test]
fn a_copy_of_another_name_stays_in_bounds() {
    assert_eq!(copy_name("Notes", 2), "Notes-copy-2");
    let long = "\u{e9}".repeat(MAX_NAME);
    let copy = copy_name(&long, 1);
    assert!(copy.len() <= MAX_NAME && copy.ends_with("-copy"));
    assert!(project_name(&copy).is_ok());
}
