use super::*;

#[test]
fn slots_are_bounded_per_user_and_in_total_for_each_pool() {
    let mut slots = Slots::default();
    for _ in 0..Pool::Triage.per_user() {
        assert_eq!(slots.take(Pool::Triage, "alice"), Ok(()));
    }
    assert_eq!(slots.take(Pool::Triage, "alice"), Err(Busy));
    slots.give_back(Pool::Triage, "alice");
    assert_eq!(slots.take(Pool::Triage, "alice"), Ok(()));

    let mut slots = Slots::default();
    let users: Vec<String> = (0..Pool::Triage.total())
        .map(|n| format!("user-{n}"))
        .collect();
    for user in &users {
        assert_eq!(slots.take(Pool::Triage, user), Ok(()));
    }
    assert_eq!(slots.take(Pool::Triage, "one-more"), Err(Busy));
    slots.give_back(Pool::Triage, &users[0]);
    assert_eq!(slots.take(Pool::Triage, "one-more"), Ok(()));
    assert_eq!(slots.running(Pool::Triage, &users[0]), 0);
}
