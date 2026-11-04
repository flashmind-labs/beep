//! Which online node forwards a repository's webhook: the current one while it can, else project
//! machines first, then other nodes, in key order, skipping recent refusals.

use iroh::EndpointId;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// How long a node that could not forward is passed over.
pub(super) const REFUSED_FOR: Duration = Duration::from_secs(10 * 60);

/// A node online now that forwards webhooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Candidate {
    pub(super) id: EndpointId,
    /// One of the project's own machines (home checkout or import target).
    pub(super) of_project: bool,
}

/// `current` while still a candidate, else the first candidate not refused within `REFUSED_FOR`.
pub(super) fn pick(
    candidates: &[Candidate],
    current: Option<EndpointId>,
    refused: &HashMap<EndpointId, Instant>,
    now: Instant,
) -> Option<EndpointId> {
    let fresh_refusal = |id: &EndpointId| {
        refused
            .get(id)
            .is_some_and(|at| now.saturating_duration_since(*at) < REFUSED_FOR)
    };
    if let Some(current) = current
        && candidates.iter().any(|candidate| candidate.id == current)
        && !fresh_refusal(&current)
    {
        return Some(current);
    }
    let mut ordered: Vec<&Candidate> = candidates.iter().collect();
    ordered.sort_by_key(|candidate| (!candidate.of_project, *candidate.id.as_bytes()));
    ordered
        .into_iter()
        .map(|candidate| candidate.id)
        .find(|id| !fresh_refusal(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn node(seed: u8) -> EndpointId {
        SecretKey::from_bytes(&[seed; 32]).public()
    }

    #[test]
    fn the_current_owner_keeps_the_repository() {
        let (a, b) = (node(1), node(2));
        let candidates = [
            Candidate {
                id: a,
                of_project: true,
            },
            Candidate {
                id: b,
                of_project: false,
            },
        ];
        let now = Instant::now();
        let none = HashMap::new();
        assert_eq!(pick(&candidates, None, &none, now), Some(a));
        assert_eq!(pick(&candidates, Some(b), &none, now), Some(b));
        // Gone offline: the next one takes it.
        assert_eq!(pick(&candidates[..1], Some(b), &none, now), Some(a));
    }

    #[test]
    fn project_machines_come_first_and_refusals_are_passed_over_for_a_while() {
        let (a, b, c) = (node(1), node(2), node(3));
        let candidates = [
            Candidate {
                id: a,
                of_project: false,
            },
            Candidate {
                id: b,
                of_project: true,
            },
            Candidate {
                id: c,
                of_project: false,
            },
        ];
        let now = Instant::now();
        assert_eq!(pick(&candidates, None, &HashMap::new(), now), Some(b));
        let refused = HashMap::from([(b, now)]);
        let first_other = if a.as_bytes() < c.as_bytes() { a } else { c };
        assert_eq!(pick(&candidates, None, &refused, now), Some(first_other));
        assert_eq!(
            pick(&candidates, None, &refused, now + REFUSED_FOR),
            Some(b),
            "tried again later"
        );
        let all = HashMap::from([(a, now), (b, now), (c, now)]);
        assert_eq!(pick(&candidates, None, &all, now), None);
    }
}
