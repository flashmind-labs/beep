use super::*;
use crate::agents::skill_library::store::tests::{put, run_git};
use beep_proto::node::mesh_state::Pick;

const PROJECT: &str = "acme/app";

fn library(global: Pick, project: Pick) -> SkillLibrary {
    SkillLibrary {
        url: "https://example.com/skills.git".into(),
        revision: "r".into(),
        skills: Vec::new(),
        global,
        projects: [(PROJECT.to_owned(), project)].into_iter().collect(),
    }
}

fn names(names: &[&str]) -> Pick {
    Pick::Only(names.iter().map(|name| name.to_string()).collect())
}

fn read(path: &Path) -> String {
    fs::read_to_string(path.join("SKILL.md")).unwrap()
}

#[test]
fn picked_skills_install_update_and_uninstall_without_losing_edits() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let node = temp.path().join("node");
    let store = temp.path().join("store");
    let checkout = temp.path().join("app");
    fs::create_dir_all(home.join(".claude")).unwrap();
    fs::create_dir_all(&checkout).unwrap();
    run_git(&checkout, &["init", "--quiet"]);
    let exclude_file = checkout.join(".git/info/exclude");
    fs::write(&exclude_file, "mine/\n").unwrap();
    for name in ["pdf", "notes", "style"] {
        put(&store, name, &format!("The {name} skill."), "v1");
    }
    // Already there: the same `notes`, and a `style` of the user's own.
    put(&home.join(".claude/skills"), "notes", "The notes skill.", "v1");
    put(&home.join(".claude/skills"), "style", "Mine.", "v1");
    let global = disk::targets(&home);
    let checkouts = [Checkout {
        project: PROJECT.into(),
        folder: checkout.clone(),
        keep_out_of_git: true,
    }];
    let ready = |global: Pick, project: Pick| {
        vec![Ready {
            name: "lib".into(),
            library: library(global, project),
            store: Some(store.clone()),
        }]
    };
    let mut record = Record::load(&node).unwrap();

    let problems = apply(
        &mut record,
        wants(&ready(Pick::All, names(&["pdf"])), &global, &checkouts),
        &checkouts,
    );
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(problems[0].skill.as_deref(), Some("style"));
    assert_eq!(problems[0].message, "a skill named style is already there");
    assert!(read(&home.join(".claude/skills/style")).contains("Mine."));
    assert!(read(&home.join(".claude/skills/pdf")).contains("v1"));
    assert!(record.installs.contains_key(&home.join(".claude/skills/notes")));
    assert_eq!(record.global_names(), ["notes", "pdf"].map(String::from).into());

    for rel in [".claude/skills/pdf", ".agents/skills/pdf"] {
        assert!(read(&checkout.join(rel)).contains("v1"), "{rel}");
    }

    let exclude = fs::read_to_string(&exclude_file).unwrap();
    assert!(exclude.starts_with("mine/\n"), "{exclude}");
    assert!(exclude.contains("/.claude/skills/pdf/\n/.agents/skills/pdf/\n"), "{exclude}");
    assert_eq!(run_git(&checkout, &["status", "--porcelain"]), "");

    // Saved and read back alike.
    record.save().unwrap();
    assert_eq!(Record::load(&node).unwrap(), record);

    // The library updates; the global copy was edited here, the project's were not.
    put(&store, "pdf", "The pdf skill.", "v2");
    put(&home.join(".claude/skills"), "pdf", "The pdf skill.", "my edit");
    let problems = apply(
        &mut record,
        wants(&ready(Pick::All, names(&["pdf"])), &global, &checkouts),
        &checkouts,
    );
    assert!(read(&home.join(".claude/skills/pdf")).contains("my edit"));
    assert!(read(&checkout.join(".claude/skills/pdf")).contains("v2"));
    assert!(
        problems
            .iter()
            .any(|p| p.message == "pdf was edited here, so it is not updated" && p.place == "global"),
        "{problems:?}"
    );

    // Nothing picked: unedited copies go, the edited one stays, and the block goes.
    let problems = apply(
        &mut record,
        wants(&ready(Pick::None, Pick::None), &global, &checkouts),
        &checkouts,
    );
    assert!(problems.is_empty(), "{problems:?}");
    assert!(record.installs.is_empty(), "{:?}", record.installs);
    assert!(read(&home.join(".claude/skills/pdf")).contains("my edit"));
    assert!(read(&home.join(".claude/skills/style")).contains("Mine."));
    assert!(!home.join(".claude/skills/notes").exists());
    assert!(!checkout.join(".claude/skills/pdf").exists());
    assert!(!checkout.join(".agents/skills/pdf").exists());
    assert_eq!(fs::read_to_string(&exclude_file).unwrap(), "mine/\n");
}

#[test]
fn a_library_that_cannot_be_fetched_keeps_what_it_installed() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let store = temp.path().join("store");
    fs::create_dir_all(home.join(".claude")).unwrap();
    put(&store, "pdf", "The pdf skill.", "v1");
    let global = disk::targets(&home);
    let mut lib = Ready {
        name: "lib".into(),
        library: library(Pick::All, Pick::None),
        store: Some(store),
    };
    let mut record = Record::load(temp.path()).unwrap();
    apply(&mut record, wants(std::slice::from_ref(&lib), &global, &[]), &[]);
    assert_eq!(record.installs.len(), 1);

    lib.store = None;
    lib.library.skills = vec![beep_proto::node::mesh_state::LibrarySkill {
        name: "pdf".into(),
        description: String::new(),
    }];
    apply(&mut record, wants(std::slice::from_ref(&lib), &global, &[]), &[]);
    assert_eq!(record.installs.len(), 1);
    assert!(home.join(".claude/skills/pdf/SKILL.md").exists());

    // A library removed from the mesh uninstalls.
    apply(&mut record, Vec::new(), &[]);
    assert!(record.installs.is_empty());
    assert!(!home.join(".claude/skills/pdf").exists());
}

#[test]
fn the_exclude_block_is_replaced_and_lines_outside_it_stay() {
    let lines = ["/.claude/skills/a/".to_owned()];
    let once = with_block("one\ntwo", &lines);
    assert_eq!(
        once,
        format!("one\ntwo\n{BLOCK_START}\n/.claude/skills/a/\n{BLOCK_END}\n")
    );
    assert_eq!(with_block(&once, &lines), once);
    let more = format!("{once}three\n");
    assert_eq!(
        with_block(&more, &["/x/".to_owned()]),
        format!("one\ntwo\nthree\n{BLOCK_START}\n/x/\n{BLOCK_END}\n")
    );
    assert_eq!(with_block(&more, &[]), "one\ntwo\nthree\n");
    assert_eq!(with_block("one", &[]), "one");
}
