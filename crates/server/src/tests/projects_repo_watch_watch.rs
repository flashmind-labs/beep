use super::*;
use uuid::Uuid;

#[test]
fn only_viewed_stale_copies_are_due_and_old_ones_are_dropped() {
    let watch = RepoWatch::default();
    let project = ProjectId(Uuid::from_u128(1));
    let start = Instant::now();
    let entry = watch.entry("alice", project);
    entry.lists.ask(start, false);
    entry.lists.timing().synced(start, true);
    assert!(watch.due(start + Duration::from_secs(60)).is_empty());
    let due = watch.due(start + Duration::from_secs(200));
    assert_eq!(due.len(), 1);
    assert!(due[0].lists && !due[0].runs, "runs were never asked for");
    // Nobody asked for two hours: the copy goes.
    assert!(
        watch
            .due(start + Duration::from_secs(3 * 60 * 60))
            .is_empty()
    );
    assert!(
        watch
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
    );
}
