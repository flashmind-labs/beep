use super::*;
use crate::lenient;
use serde::Serialize;

#[test]
fn report_request_reads_by_position() {
    let named = rmp_serde::to_vec(&Request {
        since: 7,
        max_bytes: 9,
        whole: true,
    })
    .expect("encodes");
    let positional = rmp_serde::to_vec(&(7u64, 9u32, true)).expect("encodes");
    assert_eq!(named, positional);
    // A request from before `whole` asks for the newest `max_bytes`.
    let older = rmp_serde::to_vec(&(7u64, 9u32)).expect("encodes");
    let read: Request = lenient::from_slice(&older).expect("reads");
    assert!(!read.whole);
}

#[test]
fn a_longer_report_reply_still_reads() {
    #[derive(Serialize)]
    enum Newer {
        Share {
            log: String,
            status: String,
            version: String,
            os: String,
            os_version: String,
            later: String,
        },
    }
    let bytes = rmp_serde::to_vec(&Newer::Share {
        log: "l".into(),
        status: "s".into(),
        version: "v".into(),
        os: "o".into(),
        os_version: "ov".into(),
        later: "x".into(),
    })
    .expect("encodes");
    let reply: Reply = lenient::from_slice(&bytes).expect("reads");
    assert_eq!(
        reply,
        Reply::Share {
            log: "l".into(),
            status: "s".into(),
            version: "v".into(),
            os: "o".into(),
            os_version: "ov".into(),
        }
    );
}
