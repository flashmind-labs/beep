//! What the skills lists of the mesh's machines say together, name by name.
//!
//! Each copy carries the version its machine last synced and whether it was
//! edited there since (one edit past that version). With no clash the copy
//! with the most edits wins and every other copy takes it: an edit spreads,
//! and a copy edited here waits for the others to take it. Two copies clash,
//! and nothing changes for the name, when both were edited, when one was
//! edited from a version older than the other's, or when neither was edited
//! and they differ at the same version (made apart).
//!
//! A copy the user kept to end a conflict is a version of its own: every
//! copy from an older version, edited or not, takes it with no clash.

use super::disk::Base;
use beep_proto::node::skills::{Conflict, Held, Listed};
use iroh::EndpointId;
use std::collections::{BTreeMap, BTreeSet};

/// One machine's list.
pub(super) struct Holder<'a> {
    pub id: EndpointId,
    pub machine: &'a str,
    /// This machine's own.
    pub here: bool,
    pub listed: &'a [Listed],
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Outcome {
    pub conflicts: Vec<Conflict>,
    /// Copies this machine takes.
    pub takes: Vec<Take>,
    /// Names whose copy here every machine that synced it holds: the copy
    /// they synced, to record here.
    pub synced: Vec<Synced>,
}

/// The winning copy of a name, for this machine to take.
#[derive(Debug, PartialEq)]
pub(super) struct Take {
    pub name: String,
    pub base: Base,
    /// The copies here it replaces; none when the name is new here.
    pub old: Vec<Listed>,
}

#[derive(Debug, PartialEq)]
pub(super) struct Synced {
    pub name: String,
    pub base: Base,
}

/// One copy with where it is.
struct Copy<'a> {
    id: EndpointId,
    machine: &'a str,
    here: bool,
    listed: &'a Listed,
}

impl Copy<'_> {
    /// The edits it has been through, its own among them.
    fn edits(&self) -> u64 {
        self.listed.ver + u64::from(self.listed.edited)
    }
}

/// Whether `x` and `y` differ in a way no edit order explains.
fn clash(x: &Listed, y: &Listed) -> bool {
    x.hash != y.hash
        && match (x.edited, y.edited) {
            (true, true) => true,
            (true, false) => y.ver > x.ver,
            (false, true) => x.ver > y.ver,
            (false, false) => x.ver == y.ver,
        }
}

/// Compares every list, this machine's among them. `lacks` says whether this
/// machine lacks a name and may take it.
pub(super) fn compare(holders: &[Holder<'_>], lacks: impl Fn(&str) -> bool) -> Outcome {
    let mut by_name: BTreeMap<&str, Vec<Copy<'_>>> = BTreeMap::new();
    for holder in holders {
        for listed in holder.listed {
            by_name.entry(&listed.name).or_default().push(Copy {
                id: holder.id,
                machine: holder.machine,
                here: holder.here,
                listed,
            });
        }
    }
    let mut outcome = Outcome::default();
    for (name, copies) in by_name {
        // Copies from before the latest pick take it, whatever they are.
        let picked = copies
            .iter()
            .filter(|copy| copy.listed.kept)
            .map(|copy| copy.listed.ver)
            .max();
        let live: Vec<&Copy<'_>> = copies
            .iter()
            .filter(|copy| picked.is_none_or(|ver| copy.listed.ver >= ver))
            .collect();
        let clashes = live
            .iter()
            .enumerate()
            .any(|(i, x)| live[i + 1..].iter().any(|y| clash(x.listed, y.listed)));
        if clashes {
            outcome.conflicts.push(conflict(name, &copies));
            continue;
        }
        let Some(winner) = live.iter().max_by_key(|copy| copy.edits()) else {
            continue;
        };
        let base = Base {
            hash: winner.listed.hash.clone(),
            ver: winner.edits(),
            kept: winner.listed.kept,
        };
        let old: Vec<Listed> = copies
            .iter()
            .filter(|copy| copy.here && copy.listed.hash != base.hash)
            .map(|copy| copy.listed.clone())
            .collect();
        let held = copies.iter().any(|copy| copy.here);
        if !old.is_empty() || (!held && lacks(name)) {
            outcome.takes.push(Take {
                name: name.to_owned(),
                base,
                old,
            });
        } else if held
            && let Some(ver) = copies
                .iter()
                .filter(|copy| !copy.listed.edited && copy.listed.hash == base.hash)
                .map(|copy| copy.listed.ver)
                .max()
        {
            // Another machine took this copy, so an edit made here counts
            // as synced.
            outcome.synced.push(Synced {
                name: name.to_owned(),
                base: Base {
                    hash: base.hash,
                    ver,
                    kept: base.kept,
                },
            });
        }
    }
    outcome
}

/// The conflict of `name`, told by where each content is.
fn conflict(name: &str, copies: &[Copy<'_>]) -> Conflict {
    let mut contents: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    let mut here = Vec::new();
    for copy in copies {
        let at = contents.entry(&copy.listed.hash).or_default();
        for folder in &copy.listed.folders {
            at.push((copy.machine, folder));
            if copy.here {
                here.push(format!("{} {folder}", copy.machine));
            }
        }
    }
    // A machine that holds two contents is named with its folder.
    let mut machines_of: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (hash, list) in &contents {
        for (machine, _) in list {
            machines_of.entry(machine).or_default().insert(hash);
        }
    }
    let place = |machine: &str, folder: &str| {
        if machines_of
            .get(machine)
            .is_some_and(|hashes| hashes.len() > 1)
        {
            format!("{machine} {folder}")
        } else {
            machine.to_owned()
        }
    };
    let mut places = Vec::new();
    let mut lines = Vec::new();
    for list in contents.values() {
        if let Some((machine, folder)) = list.first() {
            places.push(place(machine, folder));
        }
        lines.extend(
            list.iter()
                .map(|(machine, folder)| format!("{machine} {folder}")),
        );
    }
    places.sort();
    places.dedup();
    lines.sort();
    here.sort();
    let mut held: Vec<Held> = copies
        .iter()
        .flat_map(|copy| {
            copy.listed.folders.iter().map(|folder| Held {
                machine: copy.id,
                label: copy.machine.to_owned(),
                folder: folder.clone(),
                hash: copy.listed.hash.clone(),
                changed: copy.listed.changed,
                bytes: copy.listed.bytes,
            })
        })
        .collect();
    held.sort_by(|a, b| (&a.label, &a.folder).cmp(&(&b.label, &b.folder)));
    Conflict {
        name: name.to_owned(),
        copies: lines,
        places,
        here,
        held,
    }
}

#[cfg(test)]
mod tests {
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
}
