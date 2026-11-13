use super::*;

#[test]
fn the_rate_follows_the_last_ten_seconds() {
    let start = Instant::now();
    let mut meter = Meter::new(100, 100_000_000, start);
    // Fast at first, 1 MB a second for 20 seconds...
    for s in 1..=20 {
        meter.done(1_000_000, start + Duration::from_secs(s));
    }
    // ...then 100 KB a second for 20 more.
    for s in 21..=40 {
        meter.done(100_000, start + Duration::from_secs(s));
    }

    let rate = meter.rate();
    assert!((100_000..=200_000).contains(&rate), "rate {rate}");
}

#[test]
fn one_slow_file_spans_its_whole_time() {
    let start = Instant::now();
    let mut meter = Meter::new(1, 60_000_000, start);
    meter.done(60_000_000, start + Duration::from_secs(60));
    assert_eq!(meter.rate(), 1_000_000);
}

#[test]
fn failed_files_count_their_bytes_as_done() {
    // Files that were already here and failed to go in place: the bytes
    // done follow the files done, not stay at nothing.
    let start = Instant::now();
    let mut meter = Meter::new(4, 400, start);
    meter.failed(100);
    meter.failed(100);
    meter.done(100, start + Duration::from_secs(1));
    let p = meter.progress();
    assert_eq!((p.files_done, p.files_total), (3, 4));
    assert_eq!((p.bytes_done, p.bytes_total), (300, 400));
    assert_eq!(p.rate, 100);
    assert_eq!(p.eta_secs, Some(1));
}

#[test]
fn received_bytes_move_before_the_file_is_done() {
    // One large file: its bytes show as they come, with a rate, and the
    // file counts once it is in place.
    let start = Instant::now();
    let mut meter = Meter::new(2, 1000, start);
    meter.received(400, start + Duration::from_secs(4));
    let p = meter.progress();
    assert_eq!((p.files_done, p.bytes_done), (0, 400));
    assert_eq!(p.rate, 100);
    assert_eq!(p.eta_secs, Some(6));

    // A fetch tried again counts no more than the round holds.
    meter.received(5000, start + Duration::from_secs(5));
    meter.done(1000, start + Duration::from_secs(6));
    let p = meter.progress();
    assert_eq!((p.files_done, p.bytes_done), (1, 1000));
}

#[test]
fn failed_paths_wait_first_and_the_list_is_bounded() {
    let ahead: Vec<String> = (0..100).map(|i| format!("f{i}")).collect();
    let out = waiting(&["bad".into()], &ahead);
    assert_eq!(out.len(), MAX_WAITING);
    assert_eq!(out[0], "bad");
    assert_eq!(out[1], "f0");
}
