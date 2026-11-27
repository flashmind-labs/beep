//! Which pull requests and issues began to ask for the user between two reads of the git hosts,
//! and which stopped.

use std::collections::HashSet;

/// What asks for the user on an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Flag {
    Review,
    Assigned,
}

/// An item of a git host as the diff reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Marked {
    pub(crate) host: String,
    pub(crate) repo: String,
    pub(crate) number: u64,
    pub(crate) title: String,
    pub(crate) url: String,
    /// In milliseconds since the Unix epoch.
    pub(crate) updated_at: u64,
    pub(crate) review_requested: bool,
    pub(crate) assigned: bool,
}

impl Marked {
    fn flags(&self) -> impl Iterator<Item = Flag> {
        [
            (Flag::Review, self.review_requested),
            (Flag::Assigned, self.assigned),
        ]
        .into_iter()
        .filter_map(|(flag, on)| on.then_some(flag))
    }
}

/// What changed since the read before.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Changes<'a> {
    /// Items that began to ask for the user.
    pub(crate) raised: Vec<(Flag, &'a Marked)>,
    /// The flag and url of what stopped, or whose item is gone.
    pub(crate) cleared: Vec<(Flag, String)>,
}

/// The flags the last read saw. The first read of a host is its baseline: it raises nothing.
#[derive(Debug, Default)]
pub(crate) struct Seen {
    /// Host, url and flag.
    flags: HashSet<(String, String, Flag)>,
    /// The hosts that answered at least once.
    hosts: HashSet<String>,
}

impl Seen {
    /// Takes a read of `items` from `answered`, the hosts that answered. A host that did not
    /// answer keeps what it had, so its items are neither cleared nor raised when it is back.
    pub(crate) fn update<'a>(&mut self, items: &'a [Marked], answered: &[String]) -> Changes<'a> {
        let mut changes = Changes::default();
        let mut now = HashSet::new();

        for item in items {
            for flag in item.flags() {
                let key = (item.host.clone(), item.url.clone(), flag);

                if !self.flags.contains(&key) && self.hosts.contains(&item.host) {
                    changes.raised.push((flag, item));
                }

                now.insert(key);
            }
        }

        for key in &self.flags {
            if answered.contains(&key.0) {
                if !now.contains(key) {
                    changes.cleared.push((key.2, key.1.clone()));
                }
            } else {
                now.insert(key.clone());
            }
        }

        self.flags = now;
        self.hosts.extend(answered.iter().cloned());
        changes
    }
}

/// One read of the git hosts, as the diff takes it.
#[derive(Debug)]
pub(crate) struct Found {
    pub(crate) items: Vec<Marked>,
    /// The hosts that answered.
    pub(crate) answered: Vec<String>,
}

#[cfg(test)]
#[path = "../tests/projects_forge_notices.rs"]
mod tests;
