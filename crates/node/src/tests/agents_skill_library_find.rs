use super::*;
use crate::agents::skill_library::store::tests::{put, run_git};

#[cfg(unix)]
#[test]
fn skills_linked_into_a_repository_are_found_by_its_remote() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let repo = temp.path().join("repo");
    let skills = home.join(".claude/skills");
    fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "--quiet"]);
    run_git(
        &repo,
        &["remote", "add", "origin", "https://example.com/skills.git"],
    );
    put(&repo, "skills/pdf", "Reads PDFs.", "v1");
    put(&repo, "skills/docx", "Writes documents.", "v1");
    fs::create_dir_all(&skills).unwrap();
    symlink(repo.join("skills/pdf"), skills.join("pdf")).unwrap();
    symlink(repo.join("skills/docx"), skills.join("docx")).unwrap();
    put(&skills, "mine", "My own.", "v1");
    // A skill the agent's folder holds as part of a dotfiles repository is no library.
    let codex = home.join(".codex");
    let agents = home.join(".agents");
    fs::create_dir_all(&codex).unwrap();
    fs::create_dir_all(&agents).unwrap();
    run_git(&agents, &["init", "--quiet"]);
    run_git(
        &agents,
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/dotfiles.git",
        ],
    );
    put(&agents.join("skills"), "dot", "Dotfiles.", "v1");

    let found = find(&home, &BTreeSet::new());
    assert_eq!(
        found,
        [FoundLibrary {
            url: "https://example.com/skills.git".into(),
            skills: vec!["docx".into(), "pdf".into()],
            folders: vec!["~/.claude/skills".into()],
        }]
    );

    let ours = [skills.join("docx")].into_iter().collect();
    assert_eq!(find(&home, &ours)[0].skills, ["pdf"]);
}
