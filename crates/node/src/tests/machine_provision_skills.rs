use super::*;

const SKILL: Skill = Skill {
    name: "github",
    text: "---\nname: github\ndescription: Use for GitHub.\n---\n\n# GitHub\n\nUse gh.\n",
};
const NEWER: Skill = Skill {
    name: "github",
    text: "---\nname: github\ndescription: Use for GitHub.\n---\n\n# GitHub\n\nUse gh, newer.\n",
};

fn record(dir: &Path) -> Installed {
    Installed::load(&dir.join("node/installed-skills")).unwrap()
}

#[test]
fn beep_updates_its_own_file_and_keeps_an_edited_one() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let mut installed = record(dir);
    let path = dir.join("github/SKILL.md");
    assert_eq!(
        install(dir, &SKILL, &mut installed).unwrap(),
        Outcome::Written
    );
    assert_eq!(
        install(dir, &SKILL, &mut installed).unwrap(),
        Outcome::Current
    );
    assert_eq!(
        install(dir, &NEWER, &mut installed).unwrap(),
        Outcome::Written
    );
    assert!(fs::read_to_string(&path).unwrap().contains("newer"));

    // The user edits it: Beep leaves it alone from then on.
    let edited = fs::read_to_string(&path)
        .unwrap()
        .replace("newer", "my way");
    fs::write(&path, &edited).unwrap();
    assert_eq!(install(dir, &SKILL, &mut installed).unwrap(), Outcome::Kept);
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
}

#[test]
fn a_users_own_skill_is_kept() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let path = dir.join("github/SKILL.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "my own github skill\n").unwrap();
    assert_eq!(
        install(dir, &SKILL, &mut record(dir)).unwrap(),
        Outcome::Kept
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "my own github skill\n");
}

#[test]
fn a_skill_the_user_deleted_stays_deleted() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let path = dir.join("github/SKILL.md");
    assert_eq!(
        install(dir, &SKILL, &mut record(dir)).unwrap(),
        Outcome::Written
    );
    fs::remove_file(&path).unwrap();
    // A later node process reads the record back.
    assert_eq!(
        install(dir, &NEWER, &mut record(dir)).unwrap(),
        Outcome::Deleted
    );
    assert!(!path.exists());
}

#[test]
fn the_shipped_skills_have_front_matter() {
    for skill in SKILLS {
        assert!(front_matter_end(skill.text) > 0, "{}", skill.name);
    }
}
