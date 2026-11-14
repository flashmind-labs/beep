use super::*;

#[test]
fn restarts_wait_longer_while_the_node_keeps_failing() {
    let mut restarts = Restarts::default();
    let quick = Duration::from_secs(1);
    let pauses: Vec<u64> = (0..10)
        .map(|_| restarts.after(quick).as_secs())
        .collect();
    assert_eq!(pauses, [2, 4, 8, 16, 32, 64, 128, 256, 300, 300]);
    assert_eq!(restarts.after(SETTLED), Duration::from_secs(2));
}

#[test]
fn the_kept_distributions_are_saved_in_the_nodes_folder() {
    let dir = tempfile::tempdir().expect("a folder");
    assert!(read(dir.path()).expect("nothing saved").is_empty());
    let names: Vec<DistroName> = ["Ubuntu", "Debian", "Ubuntu"]
        .iter()
        .map(|name| name.parse().expect("a name"))
        .collect();
    write(dir.path(), &names).expect("saved");
    let read: Vec<String> = read(dir.path())
        .expect("read")
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(read, ["Debian", "Ubuntu"]);
}
