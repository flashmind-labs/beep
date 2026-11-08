use super::*;

#[test]
fn lower_round_trip_first_and_unknown_last() {
    let ms = Duration::from_millis;
    let mut machines = [
        ("far", Some(ms(80))),
        ("unknown", None),
        ("here", Some(Duration::ZERO)),
        ("near", Some(ms(4))),
        ("also unknown", None),
    ];
    by_rtt(&mut machines, |(_, rtt)| *rtt);
    let order: Vec<&str> = machines.iter().map(|(name, _)| *name).collect();
    assert_eq!(order, ["here", "near", "far", "unknown", "also unknown"]);
}
