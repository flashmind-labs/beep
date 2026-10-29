//! What a client can read.
//!
//! Phones update on their own schedule, so the server is often newer than
//! the app talking to it, and rmp_serde refuses a reply with an element it
//! does not expect. Every connection opens with `Hello`, which carries the
//! client's `WIRE`. The server checks `ClientInfo::supports` before sending
//! anything an older wire cannot decode.

use crate::{Hello, Platform};

/// The newest wire revision this crate reads. Raise it together with a new
/// `Feature`, and run the compat test with `BEEP_WIRE_BLESS=1` to freeze the
/// new revision's fixtures.
pub const WIRE: u32 = 7;

/// A reply shape newer than some clients: a streamed enum variant, or
/// anything else only the `WIRE` it names can decode. Declare each as an
/// associated const, `pub const POLLS: Feature = Feature(2);`, and mark a new
/// `wire_enum!` variant with `since Feature::POLLS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature(pub(crate) u32);

impl Feature {
    /// Draws `show_chart` and `show_stock_chart` results and can ask
    /// `StockChart`. Only the macOS app does.
    pub const CHARTS: Feature = Feature(2);

    /// Draws `show_cards` results.
    pub const CARDS: Feature = Feature(3);

    /// Answers `DeviceQuery::MacFiles`. Only the macOS app does.
    pub const MAC_FILES: Feature = Feature(4);

    /// Plays voice notes: reads `RunEvent::Transcript`, which a run started
    /// from a `Chat` with `voice` streams. Only the macOS app does.
    pub const VOICE: Feature = Feature(5);

    /// Asks `ListMachines` and `ListMemoryScopes`: shows the machines running
    /// beep-node and what each memory is about.
    pub const NODES: Feature = Feature(6);

    /// Asks `Transcribe` for live words while the user talks. Only the
    /// macOS app does.
    pub const LIVE_VOICE: Feature = Feature(7);

    /// The first wire revision that reads it.
    pub const fn wire(self) -> u32 {
        self.0
    }
}

/// The app on the other end of a connection, as its `Hello` gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientInfo {
    pub wire: u32,
    pub platform: Option<Platform>,
    /// The app's own version, such as "0.1.0 (142)".
    pub version: Option<String>,
}

impl ClientInfo {
    /// A client from before `Hello`. It reads wire 1, the shapes frozen in
    /// `tests/wire/1`.
    pub const LEGACY: Self = Self {
        wire: 1,
        platform: None,
        version: None,
    };

    pub fn supports(&self, feature: Feature) -> bool {
        self.wire >= feature.wire()
    }
}

impl From<Hello> for ClientInfo {
    fn from(hello: Hello) -> Self {
        Self {
            wire: hello.wire,
            platform: hello.platform,
            version: hello.version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_feature_needs_its_wire() {
        let newer = Feature(WIRE + 1);
        assert!(!ClientInfo::LEGACY.supports(newer));
        let current = ClientInfo {
            wire: WIRE + 1,
            ..ClientInfo::LEGACY
        };
        assert!(current.supports(newer));
        assert!(current.supports(Feature(1)));
    }
}
