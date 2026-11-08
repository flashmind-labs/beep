use super::*;

fn listed(name: &str, hash: &str, ver: u64, edited: bool) -> Listed {
    Listed {
        name: name.to_owned(),
        hash: hash.to_owned(),
        folders: vec!["~/.claude/skills".to_owned()],
        ver,
        edited,
        kept: false,
        changed: 0,
        bytes: 0,
    }
}

fn two(a: &[Listed], b: &[Listed]) -> Outcome {
    let holders = [
        Holder {
            id: iroh::SecretKey::generate().public(),
            machine: "box-a",
            here: true,
            listed: a,
        },
        Holder {
            id: iroh::SecretKey::generate().public(),
            machine: "box-b",
            here: false,
            listed: b,
        },
    ];
    compare(&holders, |_| true)
}

#[test]
fn an_edit_spreads_and_two_edits_clash() {
    // B edited the copy both synced: A takes it.
    let out = two(
        &[listed("n", "old", 0, false)],
        &[listed("n", "new", 0, true)],
    );
    assert_eq!(
        out.takes[0].base,
        Base {
            hash: "new".into(),
            ver: 1,
            kept: false
        }
    );
    assert!(out.conflicts.is_empty());
    // A edited it: A waits for B.
    let out = two(
        &[listed("n", "new", 0, true)],
        &[listed("n", "old", 0, false)],
    );
    assert!(out.takes.is_empty() && out.conflicts.is_empty());
    // B took A's edit: A's counts as synced.
    let out = two(
        &[listed("n", "new", 0, true)],
        &[listed("n", "new", 1, false)],
    );
    assert_eq!(
        out.synced[0].base,
        Base {
            hash: "new".into(),
            ver: 1,
            kept: false
        }
    );
    // A lagged behind a later version: it catches up.
    let out = two(
        &[listed("n", "old", 0, false)],
        &[listed("n", "new", 1, false)],
    );
    assert_eq!(out.takes[0].base.hash, "new");
    // Both edited, or A edited a copy older than B's: a conflict.
    for b in [listed("n", "b", 0, true), listed("n", "b", 1, false)] {
        let out = two(&[listed("n", "a", 0, true)], &[b]);
        assert!(out.takes.is_empty());
        assert_eq!(
            out.conflicts[0].title(),
            "Skill n differs on box-a and box-b"
        );
        assert_eq!(out.conflicts[0].held.len(), 2);
    }
}

#[test]
fn a_kept_copy_replaces_older_copies_even_edited_ones() {
    let picked = Listed {
        kept: true,
        ..listed("n", "k", 1, false)
    };
    // Two copies here edited apart, both from before the pick.
    let out = two(
        &[listed("n", "a", 0, true), listed("n", "c", 0, true)],
        std::slice::from_ref(&picked),
    );
    assert!(out.conflicts.is_empty());
    assert_eq!(
        out.takes[0].base,
        Base {
            hash: "k".into(),
            ver: 1,
            kept: true
        }
    );
    assert_eq!(out.takes[0].old.len(), 2);
    // An edit made after the pick spreads as any edit does.
    let out = two(&[listed("n", "e", 1, true)], &[picked]);
    assert!(out.takes.is_empty() && out.conflicts.is_empty());
}
