//! What the skills lists of the mesh's machines say together: the names that
//! differ somewhere, and the ones this machine lacks and can take.

use beep_proto::node::skills::{Conflict, Listed};
use std::collections::{BTreeMap, BTreeSet};

/// One machine's list.
pub(super) struct Holder<'a> {
    pub machine: &'a str,
    pub listed: &'a [Listed],
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Outcome {
    pub conflicts: Vec<Conflict>,
    /// Names this machine lacks that every machine holding them holds alike,
    /// with that content's hash.
    pub wanted: Vec<Wanted>,
}

#[derive(Debug, PartialEq)]
pub(super) struct Wanted {
    pub name: String,
    pub hash: String,
}

/// Where one copy is.
struct Copy<'a> {
    machine: &'a str,
    folder: &'a str,
}

/// The copies of one name, by content hash.
type Contents<'a> = BTreeMap<&'a str, Vec<Copy<'a>>>;

/// Compares every list, this machine's among them. `lacks` says whether this
/// machine lacks a name and may take it.
pub(super) fn compare(holders: &[Holder<'_>], lacks: impl Fn(&str) -> bool) -> Outcome {
    let mut by_name: BTreeMap<&str, Contents<'_>> = BTreeMap::new();
    for holder in holders {
        for listed in holder.listed {
            let copies = by_name
                .entry(&listed.name)
                .or_default()
                .entry(&listed.hash)
                .or_default();
            for folder in &listed.folders {
                copies.push(Copy {
                    machine: holder.machine,
                    folder,
                });
            }
        }
    }
    let mut outcome = Outcome::default();
    for (name, contents) in by_name {
        if contents.len() == 1 {
            if lacks(name)
                && let Some(hash) = contents.keys().next()
            {
                outcome.wanted.push(Wanted {
                    name: name.to_owned(),
                    hash: (*hash).to_owned(),
                });
            }
            continue;
        }
        // A machine that holds two contents is named with its folder.
        let mut machines_of: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (hash, copies) in &contents {
            for copy in copies {
                machines_of.entry(copy.machine).or_default().insert(hash);
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
        let mut copies = Vec::new();
        for list in contents.values() {
            if let Some(copy) = list.first() {
                places.push(place(copy.machine, copy.folder));
            }
            copies.extend(
                list.iter()
                    .map(|copy| format!("{} {}", copy.machine, copy.folder)),
            );
        }
        places.sort();
        places.dedup();
        copies.sort();
        outcome.conflicts.push(Conflict {
            name: name.to_owned(),
            copies,
            places,
        });
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(name: &str, hash: &str, folders: &[&str]) -> Listed {
        Listed {
            name: name.to_owned(),
            hash: hash.to_owned(),
            folders: folders.iter().map(|f| (*f).to_owned()).collect(),
        }
    }

    #[test]
    fn a_name_held_alike_is_wanted_and_one_held_two_ways_differs() {
        let a = [
            listed("notes", "1", &["~/.claude/skills", "~/.agents/skills"]),
            listed("style", "2", &["~/.claude/skills"]),
            listed("tone", "4", &["~/.claude/skills"]),
            listed("tone", "5", &["~/.agents/skills"]),
        ];
        let b = [
            listed("notes", "1", &["~/.claude/skills"]),
            listed("style", "3", &["~/.claude/skills"]),
        ];
        let holders = [
            Holder {
                machine: "box-a",
                listed: &a,
            },
            Holder {
                machine: "box-b",
                listed: &b,
            },
        ];
        let outcome = compare(&holders, |name| name == "notes");
        assert_eq!(
            outcome.wanted,
            [Wanted {
                name: "notes".into(),
                hash: "1".into()
            }]
        );
        let lines: Vec<String> = outcome.conflicts.iter().map(Conflict::title).collect();
        assert_eq!(
            lines,
            [
                "Skill style differs on box-a and box-b",
                "Skill tone differs on box-a ~/.agents/skills and box-a ~/.claude/skills",
            ]
        );
    }
}
