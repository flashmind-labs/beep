//! `report/1`: what a node shares with a problem report. The desktop sends one
//! `Request` per stream and the node answers with one `Reply`.

use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "report/1";
/// The most log bytes a node gives.
pub const MAX_BYTES: u32 = 1024 * 1024;

/// What the desktop sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// Only log lines from this Unix time (seconds) on.
    pub since: u64,
    /// The most log bytes wanted, up to `MAX_BYTES`.
    pub max_bytes: u32,
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    /// What the node shares, nothing else leaves it.
    Share {
        log: String,
        status: String,
        version: String,
        os: String,
        os_version: String,
    },
    /// It cannot share, in words for the user.
    Failed { reason: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lenient;
    use serde::Serialize;

    #[test]
    fn report_request_reads_by_position() {
        let named = rmp_serde::to_vec(&Request {
            since: 7,
            max_bytes: 9,
        })
        .expect("encodes");
        let positional = rmp_serde::to_vec(&(7u64, 9u32)).expect("encodes");
        assert_eq!(named, positional);
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
}
