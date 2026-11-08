use super::*;
use beep_proto::{ResearchEntry, ResearchStatus};

fn research(seqs: &[i64], draft: &str) -> Research {
    Research {
        id: Uuid::nil(),
        topic: "Solid-state batteries".into(),
        status: ResearchStatus::Researching {},
        agents: Vec::new(),
        entries: seqs
            .iter()
            .map(|&seq| ResearchEntry {
                seq,
                agent: "a".into(),
                body: ResearchEntryBody::Said {
                    to: None,
                    content: format!("entry {seq}"),
                },
                timestamp: seq,
            })
            .collect(),
        sources: Vec::new(),
        draft: draft.into(),
        started_at: 0,
        finished_at: None,
        round: 1,
    }
}

fn seqs(view: &ResearchView) -> Vec<i64> {
    view.entries.iter().map(|entry| entry.seq).collect()
}

#[test]
fn keeps_every_entry_and_asks_only_for_newer_ones() {
    let cache = Cache::default();
    let session = Uuid::new_v4();
    assert_eq!(cache.last_seq(session), 0);

    let first = cache.merge(session, research(&[1, 2], ""), 0);
    assert_eq!(seqs(&first), [1, 2]);
    assert_eq!(cache.last_seq(session), 2);

    // The next poll brings only entry 3; the window already has 1 and 2.
    let next = cache.merge(session, research(&[3], "# Paper"), 2);
    assert_eq!(seqs(&next), [3]);
    assert!(next.draft_html.contains("Paper"));

    // A window opened later gets all of it without asking.
    let reopened = cache.view(session).expect("the research was cached");
    assert_eq!(seqs(&reopened), [1, 2, 3]);
    assert_eq!(reopened.draft, "# Paper");
}
