use super::*;

#[test]
fn a_step_that_runs_too_long_is_named() {
    let _running = enter(Step::SyncApply);
    let later = busy(Step::SyncApply).since_ms.load(Ordering::Relaxed)
        + u64::try_from(STALL.as_millis()).expect("small");
    let what = stalls(later).expect("a stall");
    assert!(
        what.contains("applying the mesh's sync set has run for"),
        "{what}"
    );
}
