use super::*;
use crate::projects::model::CHECK_EVERY;
use chrono::TimeDelta;
use uuid::Uuid;

fn t0() -> DateTime<Utc> {
    DateTime::from_timestamp(1_800_000_000, 0).expect("a time")
}

#[test]
fn checks_repeat_every_ten_minutes() {
    let project = Due::Check(ProjectId(Uuid::new_v4()));
    let mut schedule = Schedule::default();
    let every = TimeDelta::from_std(CHECK_EVERY).expect("ten minutes");
    schedule.arm(project, t0() + every);
    assert!(
        schedule
            .take_due(t0() + every - TimeDelta::seconds(1))
            .is_empty()
    );
    assert_eq!(schedule.take_due(t0() + every), vec![project]);
    // Taken once; re-armed by whoever handles it.
    assert!(schedule.take_due(t0() + every).is_empty());
    assert_eq!(schedule.next(), None);
}

#[test]
fn idle_is_due_after_thirty_minutes() {
    let worker = Due::Idle(WorkerId(Uuid::new_v4()));
    let check = Due::Check(ProjectId(Uuid::new_v4()));
    let mut schedule = Schedule::default();
    let idle = TimeDelta::from_std(super::super::model::IDLE_CLOSE).expect("thirty minutes");
    schedule.arm(worker, t0() + idle);
    schedule.arm(check, t0() + TimeDelta::minutes(10));
    assert_eq!(schedule.next(), Some(t0() + TimeDelta::minutes(10)));
    assert!(
        schedule
            .take_due(t0() + idle - TimeDelta::seconds(1))
            .contains(&check)
    );
    assert!(schedule.take_due(t0() + TimeDelta::minutes(29)).is_empty());
    assert_eq!(schedule.take_due(t0() + idle), vec![worker]);
    // Arming again moves it rather than adding a second.
    schedule.arm(worker, t0());
    schedule.arm(worker, t0() + idle);
    assert_eq!(schedule.armed_at(&worker), Some(t0() + idle));
}
