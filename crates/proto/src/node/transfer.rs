//! `transfer/1`: a file or folder sent from one node to another directly,
//! over iroh-blobs, with the server only brokering. The server sends the
//! source an `Offer` naming the target's key; the source imports the path
//! and answers `Offered` with what to fetch and from where, and keeps the
//! offer open until the server closes the stream. The server then sends the
//! target a `Fetch`; it downloads the content with verified streaming,
//! writes it into the folder under a hidden partial name and renames it
//! into place, never replacing a file. A node offers it only when it shares
//! folders, and both the source path and the target folder must lie in them.

use iroh::{EndpointAddr, EndpointId};
use serde::{Deserialize, Serialize};
use strum::IntoStaticStr;

pub const PROTOCOL: &str = "transfer/1";

/// The most files one folder transfer carries.
pub const MAX_FILES: u64 = 10_000;
/// The most bytes one transfer carries.
pub const MAX_BYTES: u64 = 256 << 30;
/// The most numbered names tried before a transfer gives up on a name.
pub const MAX_SUFFIX: u32 = 1000;

/// What the server sends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Request {
    /// On the source: makes `path`, a file or folder, ready for the
    /// endpoint `peer` to fetch while this stream stays open.
    Offer { path: String, peer: EndpointId },
    /// On the target: fetches `content` from `from` and writes it into
    /// `folder` as `name`, or the first free name after it.
    Fetch {
        from: EndpointAddr,
        content: Content,
        /// The file or folder name at the source, one plain path part.
        name: String,
        /// The bytes the source offered, for progress.
        #[serde(default)]
        size: u64,
        /// An absolute folder that exists.
        folder: String,
    },
    /// Stops a fetch; the target removes what it wrote and answers
    /// `Cancelled`.
    Cancel {},
}

/// What the node answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Reply {
    /// Bytes hashed (source) or downloaded (target) so far.
    Progress {
        done: u64,
        total: u64,
    },
    /// The source is ready: the target fetches `content` from `from`.
    Offered {
        content: Content,
        from: EndpointAddr,
        name: String,
        size: u64,
        files: u64,
        /// Symlinks and special files a folder held that were left out.
        #[serde(default)]
        skipped: u64,
    },
    /// The target wrote it: `path` is where it landed.
    Done {
        path: String,
        size: u64,
        files: u64,
    },
    Cancelled {},
    Error {
        message: String,
    },
}

/// What a transfer carries: a blob or a collection, by its BLAKE3 root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Content {
    #[serde(with = "serde_bytes")]
    pub hash: [u8; 32],
    pub kind: ContentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "lowercase")]
pub enum ContentKind {
    /// One file, a raw blob.
    File,
    /// A folder, a collection of relative paths and their blobs.
    Folder,
}

/// The `n`th name tried for `name` when it is taken: `report.pdf`, then
/// `report (2).pdf`, `report (3).pdf`. A folder, or a file without an
/// extension (or only a leading dot), gets the number at the end.
pub fn numbered(name: &str, kind: ContentKind, n: u32) -> String {
    if n <= 1 {
        return name.to_owned();
    }
    let split = match kind {
        ContentKind::File => name.rfind('.').filter(|&at| at > 0),
        ContentKind::Folder => None,
    };
    match split {
        Some(at) => format!("{} ({n}){}", &name[..at], &name[at..]),
        None => format!("{name} ({n})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    fn round_trip<T: Serialize + for<'de> Deserialize<'de>>(value: &T) -> T {
        let bytes = rmp_serde::to_vec_named(value).expect("encodes");
        rmp_serde::from_slice(&bytes).expect("decodes")
    }

    #[test]
    fn taken_names_get_a_number_before_the_extension() {
        let file = ContentKind::File;
        assert_eq!(numbered("report.pdf", file, 1), "report.pdf");
        assert_eq!(numbered("report.pdf", file, 2), "report (2).pdf");
        assert_eq!(numbered("scene.tar.gz", file, 3), "scene.tar (3).gz");
        assert_eq!(numbered("Makefile", file, 2), "Makefile (2)");
        assert_eq!(numbered(".env", file, 2), ".env (2)");
        assert_eq!(
            numbered("renders.v2", ContentKind::Folder, 2),
            "renders.v2 (2)"
        );
    }

    #[test]
    fn requests_and_replies_round_trip() {
        let content = Content {
            hash: [7; 32],
            kind: ContentKind::Folder,
        };
        let fetch = Request::Fetch {
            from: EndpointAddr::new(SecretKey::generate().public()),
            content,
            name: "renders".into(),
            size: 42,
            folder: "/srv/project".into(),
        };
        assert_eq!(round_trip(&fetch), fetch);
        let offer = Request::Offer {
            path: "/srv/project/a.blend".into(),
            peer: SecretKey::generate().public(),
        };
        assert_eq!(round_trip(&offer), offer);
        let done = Reply::Done {
            path: "/srv/project/renders (2)".into(),
            size: 42,
            files: 3,
        };
        assert_eq!(round_trip(&done), done);
        let kind: &'static str = ContentKind::File.into();
        assert_eq!(kind, "file");
    }

    #[test]
    fn an_offer_without_skipped_still_reads() {
        #[derive(Serialize)]
        enum Older {
            Offered {
                content: Content,
                from: EndpointAddr,
                name: &'static str,
                size: u64,
                files: u64,
            },
        }
        let from = EndpointAddr::new(SecretKey::generate().public());
        let content = Content {
            hash: [1; 32],
            kind: ContentKind::File,
        };
        let bytes = rmp_serde::to_vec_named(&Older::Offered {
            content,
            from: from.clone(),
            name: "a.txt",
            size: 1,
            files: 1,
        })
        .expect("encodes");
        let reply: Reply = rmp_serde::from_slice(&bytes).expect("decodes");
        assert_eq!(
            reply,
            Reply::Offered {
                content,
                from,
                name: "a.txt".into(),
                size: 1,
                files: 1,
                skipped: 0,
            }
        );
    }
}
