use tokio::sync::watch;

use super::*;

#[test]
fn reads_one_after_another() {
    let (_tx, rx) = watch::channel(1);

    assert_eq!(read(&rx, |v| *v) + read(&rx, |v| *v), 2);
}

#[test]
#[should_panic(expected = "#686")]
fn refuses_a_read_inside_another() {
    let (_tx, rx) = watch::channel(1);

    read(&rx, |_| read(&rx, |v| *v));
}
