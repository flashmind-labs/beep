use super::*;
use std::process::Command;

/// Runs git in `dir` as a test user; its output.
pub(crate) fn run_git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(["-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Writes a skill `rel` (its folder from `root`) with `body` after its frontmatter.
pub(crate) fn put(root: &Path, rel: &str, description: &str, body: &str) {
    let dir = root.join(rel);
    fs::create_dir_all(&dir).unwrap();
    let front = if description.is_empty() {
        String::new()
    } else {
        format!("---\nname: x\ndescription: {description}\n---\n")
    };
    fs::write(dir.join(SKILL_MD), format!("{front}{body}\n")).unwrap();
}

#[test]
fn skills_are_folders_with_a_skill_md_outside_hidden_folders_and_other_skills() {
    let paths = [
        "README.md",
        "SKILL.md",
        "skills/Bundle/SKILL.md",
        "pdf/SKILL.md",
        "pdf/inner/SKILL.md",
        "group/docx/SKILL.md",
        "deep/pdf/SKILL.md",
        ".github/hidden/SKILL.md",
        "a/b/c/d/e/f/g/SKILL.md",
        ".claude/skills/blend/SKILL.md",
        ".cursor/skills/blend/SKILL.md",
        ".claude/skills/SKILL.md",
        ".claude/skills/.hidden/SKILL.md",
    ];
    let found = skill_dirs(paths.into_iter(), "bundle.git");
    assert_eq!(
        found.into_iter().collect::<Vec<_>>(),
        [
            ("Bundle".to_owned(), "skills/Bundle/".to_owned()),
            ("blend".to_owned(), ".claude/skills/blend/".to_owned()),
            ("bundle-2".to_owned(), String::new()),
            ("docx".to_owned(), "group/docx/".to_owned()),
            ("pdf".to_owned(), "pdf/".to_owned()),
        ]
    );
}

#[test]
fn a_git_library_is_inspected_and_checked_out_at_a_revision() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let node = temp.path().join("node");
    fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "--quiet"]);
    put(&repo, "skills/pdf", "Reads PDFs.", "Body");
    put(&repo, "notes", "", "# Takes notes\nMore");
    put(&repo, ".claude/skills/blend", "Renders scenes.", "Body");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "--quiet", "-m", "one"]);
    let first = run_git(&repo, &["rev-parse", "HEAD"]);
    let url = repo.to_str().unwrap();

    let (revision, skills) = inspect_git(&node, url).unwrap();
    assert_eq!(revision, first);
    assert_eq!(
        skills,
        [
            LibrarySkill {
                name: "blend".into(),
                description: "Renders scenes.".into()
            },
            LibrarySkill {
                name: "notes".into(),
                description: "Takes notes".into()
            },
            LibrarySkill {
                name: "pdf".into(),
                description: "Reads PDFs.".into()
            },
        ]
    );

    put(&repo, "skills/pdf", "Reads PDFs better.", "Body");
    run_git(&repo, &["commit", "--quiet", "-am", "two"]);
    let second = run_git(&repo, &["rev-parse", "HEAD"]);

    let root = ensure_git(&node, url, &first).unwrap();
    let text = fs::read_to_string(root.join("skills/pdf/SKILL.md")).unwrap();
    assert!(text.contains("Reads PDFs."), "{text}");
    assert_eq!(
        skills_in(&root, url).into_keys().collect::<Vec<_>>(),
        ["blend", "notes", "pdf"]
    );

    // A revision the clone has not fetched yet.
    ensure_git(&node, url, &second).unwrap();
    let text = fs::read_to_string(root.join("skills/pdf/SKILL.md")).unwrap();
    assert!(text.contains("better"), "{text}");
    assert!(inspect_git(&node, "/no/such/repository").is_err());
}

#[test]
fn a_node_without_the_clone_installs_from_a_copy_another_node_serves() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    let reader = temp.path().join("reader");
    let other = temp.path().join("other");
    fs::create_dir_all(&repo).unwrap();
    run_git(&repo, &["init", "--quiet"]);
    put(&repo, "skills/pdf", "Reads PDFs.", "Body");
    run_git(&repo, &["add", "."]);
    run_git(&repo, &["commit", "--quiet", "-m", "one"]);
    let url = repo.to_str().unwrap();
    // Served from the clone before it is checked out at the revision.
    let (revision, _) = inspect_git(&reader, url).unwrap();
    assert!(cloned(&reader, url));
    assert!(!cloned(&other, url));
    assert!(skill_at(&reader, url, "0000000", "pdf").unwrap().is_none());
    assert!(skill_at(&reader, url, &revision, "../pdf").unwrap().is_none());
    assert!(skill_at(&other, url, &revision, "pdf").unwrap().is_none());

    let files = skill_at(&reader, url, &revision, "pdf").unwrap().unwrap();
    let root = ensure_git(&reader, url, &revision).unwrap();
    assert_eq!(skill_disk::read(&root.join("skills/pdf")).unwrap().unwrap(), files);
    let old = copy_root(&other, url, "abcdef0").unwrap();
    fs::create_dir_all(&old).unwrap();
    let root = copy_root(&other, url, &revision).unwrap();
    super::super::install::write(&root.join("pdf"), &files).unwrap();
    prune_copies(&other, url, &revision);

    assert!(!old.exists());
    assert_eq!(skills_in(&root, url).into_keys().collect::<Vec<_>>(), ["pdf"]);
    assert_eq!(skill_at(&other, url, &revision, "pdf").unwrap().unwrap(), files);
    assert!(copy_root(&other, url, "../x").is_err());
}
