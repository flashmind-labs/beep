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
        let parsed = parse::markdown(skill.text);
        assert_eq!(parsed.name.as_deref(), Some(skill.name));
        assert!(!parsed.description.is_empty(), "{}", skill.name);
    }
    assert!(SKILLS.iter().any(|skill| skill.name == "beep_join_team"));
}

#[test]
fn the_brief_lists_each_skill_with_its_file() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    assert_eq!(brief(dir), None);

    install(dir, &SKILL, &mut record(dir)).unwrap();
    fs::create_dir_all(dir.join("notes")).unwrap();
    fs::write(
        dir.join("notes/SKILL.md"),
        "---\ndescription: Write notes.\n---\nBody\n",
    )
    .unwrap();
    fs::create_dir_all(dir.join("empty")).unwrap();

    let brief = brief(dir).unwrap();
    let github = format!(
        "- github: Use for GitHub. (`{}`)",
        dir.join("github/SKILL.md").display()
    );
    let notes = format!(
        "- notes: Write notes. (`{}`)",
        dir.join("notes/SKILL.md").display()
    );
    assert!(brief.ends_with(&format!("\n{github}\n{notes}")), "{brief}");
    assert!(!brief.contains("empty"));
}

#[test]
fn the_report_skill_is_listed_and_adds_the_final_response_rule() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();
    let report = SKILLS.iter().find(|skill| skill.name == REPORT).unwrap();
    install(dir, report, &mut record(dir)).unwrap();

    let text = brief(dir).unwrap();
    assert!(
        text.starts_with(&format!("## Beep skills\n\n{REPORT_RULE}")),
        "{text}"
    );
    assert!(text.contains("Beep keeps skills of its own"), "{text}");
    assert!(text.contains("- report:"), "{text}");
    assert!(!text.contains("<skill"), "{text}");
    assert!(!text.contains("# The report"), "{text}");
    assert!(!text.contains("Installed by Beep"), "{text}");

    install(dir, &SKILL, &mut record(dir)).unwrap();
    let text = brief(dir).unwrap();
    assert!(
        text.starts_with(&format!("## Beep skills\n\n{REPORT_RULE}")),
        "{text}"
    );
    assert!(text.contains("- github:"), "{text}");
    assert!(text.contains("- report:"), "{text}");

    fs::remove_file(dir.join("report/SKILL.md")).unwrap();
    let without = brief(dir).unwrap();
    assert!(!without.contains(REPORT_RULE), "{without}");
    assert!(!without.contains("- report:"), "{without}");
}
