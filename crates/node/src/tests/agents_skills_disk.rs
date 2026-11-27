use super::*;

fn skill(home: &Path, rel: &str, name: &str, text: &str) {
    let dir = home.join(rel).join(name);
    fs::create_dir_all(dir.join("refs")).unwrap();
    fs::write(dir.join("SKILL.md"), text).unwrap();
    fs::write(dir.join("refs/more.md"), "more\n").unwrap();
    fs::write(dir.join(".DS_Store"), "junk").unwrap();
}

#[test]
fn copies_alike_list_once_and_beeps_own_are_left_out() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    skill(
        home,
        ".claude/skills",
        "notes",
        "---\nname: notes\n---\nA\n",
    );
    skill(
        home,
        ".agents/skills",
        "notes",
        "---\nname: notes\n---\nA\n",
    );
    skill(home, ".pi/agent/skills", "style", "B\n");
    skill(home, ".claude/skills", "style", "C\n");
    let beeps = "---\nname: github\n---\n<!-- Installed by Beep. Beep keeps this file current until you edit it. sha256:00 -->\n";
    skill(home, ".claude/skills", "github", beeps);
    let found = scan(home, &mut Hashes::default());
    let notes: Vec<_> = found.listed.iter().filter(|l| l.name == "notes").collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].folders, ["~/.claude/skills", "~/.agents/skills"]);
    assert_eq!(found.listed.iter().filter(|l| l.name == "style").count(), 2);
    assert!(!found.listed.iter().any(|l| l.name == "github"));
    assert!(found.names.contains("github"));

    // What is fetched installs into a home with nothing by that name.
    let files = files_of(home, "notes", &notes[0].hash);
    assert_eq!(files.len(), 2, "the hidden file is left out");
    let other = tempfile::tempdir().unwrap();
    fs::create_dir_all(other.path().join(".claude")).unwrap();
    fs::create_dir_all(other.path().join(".codex")).unwrap();
    assert!(install(other.path(), "notes", &files).unwrap());
    let there = scan(other.path(), &mut Hashes::default());
    assert_eq!(there.listed.len(), 1);
    assert_eq!(there.listed[0].hash, notes[0].hash);
    assert!(!install(other.path(), "notes", &files).unwrap());
}

#[test]
fn hermes_skills_sync_both_ways_without_replacing_local_edits() {
    let source = tempfile::tempdir().unwrap();
    skill(source.path(), ".hermes/skills", "notes", "A\n");
    let listed = scan(source.path(), &mut Hashes::default()).listed;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].folders, ["~/.hermes/skills"]);
    let files = files_of(source.path(), "notes", &listed[0].hash);
    let target = tempfile::tempdir().unwrap();
    fs::create_dir_all(target.path().join(".hermes")).unwrap();
    assert!(install(target.path(), "notes", &files).unwrap());
    assert!(
        target
            .path()
            .join(".hermes/skills/notes/SKILL.md")
            .is_file()
    );
    skill(target.path(), ".hermes/skills", "notes", "Local edits\n");
    assert!(!install(target.path(), "notes", &files).unwrap());
    assert_eq!(
        fs::read_to_string(target.path().join(".hermes/skills/notes/SKILL.md")).unwrap(),
        "Local edits\n"
    );
}

#[test]
fn an_unchanged_folder_is_not_read_again() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    skill(home, ".claude/skills", "notes", "A\n");
    let mut hashes = Hashes::default();
    let first = scan(home, &mut hashes).listed[0].hash.clone();
    // Same size and time: the hash kept is listed, nothing is read.
    let file = home.join(".claude/skills/notes/SKILL.md");
    let at = fs::metadata(&file).unwrap().modified().unwrap();
    fs::write(&file, "B\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(at)
        .unwrap();
    assert_eq!(scan(home, &mut hashes).listed[0].hash, first);
    assert_ne!(scan(home, &mut Hashes::default()).listed[0].hash, first);
    // A size that changed reads the folder again.
    fs::write(&file, "edited\n").unwrap();
    let edited = scan(home, &mut hashes).listed[0].hash.clone();
    assert_ne!(edited, first);
    assert_eq!(edited, scan(home, &mut Hashes::default()).listed[0].hash);
}

#[test]
fn a_kept_copy_replaces_the_other_folders() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    skill(home, ".claude/skills", "style", "kept\n");
    skill(home, ".pi/agent/skills", "style", "edited apart\n");
    skill(home, ".agents/skills", "style", "another\n");
    let found = scan(home, &mut Hashes::default());
    assert_eq!(found.listed.len(), 3);
    let kept = found
        .listed
        .iter()
        .find(|l| l.folders == ["~/.claude/skills"])
        .unwrap();
    assert!(kept.changed > 0 && kept.bytes == 10);
    let old: Vec<Listed> = found
        .listed
        .iter()
        .filter(|l| l.hash != kept.hash)
        .cloned()
        .collect();
    keep(home, "style", &kept.hash, &old).unwrap();
    let after = scan(home, &mut Hashes::default());
    assert_eq!(after.listed.len(), 1);
    assert_eq!(after.listed[0].hash, kept.hash);
    assert_eq!(after.listed[0].folders.len(), 3);
    assert!(keep(home, "style", "gone", &[]).is_err());
}

#[test]
fn unsafe_names_and_paths_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".claude")).unwrap();
    let file = |path: &str| SkillFile {
        path: path.to_owned(),
        data: ByteBuf::from(b"x".to_vec()),
    };
    assert!(install(temp.path(), "../out", &[file("SKILL.md")]).is_err());
    assert!(install(temp.path(), "ok", &[file("../SKILL.md")]).is_err());
    assert!(install(temp.path(), "ok", &[file("a\\..\\b")]).is_err());
    assert!(!temp.path().join("out").exists());
}

#[test]
fn relevant_change_ignores_hidden_skill_entries() {
    let temp = tempfile::tempdir().unwrap();
    let codex_skills = temp.path().join(".codex/skills");
    fs::create_dir_all(&codex_skills).unwrap();
    let missing_skills = temp.path().join(".gemini/skills");
    let roots = vec![codex_skills.clone(), missing_skills];

    assert!(!relevant_change(
        &roots,
        &[
            codex_skills.join(".system/skill-creator/SKILL.md"),
            codex_skills.join(".DS_Store"),
            temp.path().join(".gemini"),
        ],
    ));
    assert!(relevant_change(
        &roots,
        &[codex_skills.join("my-skill/SKILL.md")],
    ));
}
