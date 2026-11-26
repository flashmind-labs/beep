use super::*;

#[test]
fn a_miss_counts_as_first_once_per_window() {
    let copies = Copies::default();
    let asked = || Asked {
        machine_id: "box".into(),
        folder: "/srv/widget".into(),
    };
    assert!(copies.keep(asked(), None));
    assert!(!copies.keep(asked(), None));
    let long_ago = Instant::now().checked_sub(FRESH).unwrap();
    copies.known.lock().unwrap().get_mut(&asked()).unwrap().at = long_ago;
    assert!(copies.keep(asked(), None));
}
