use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};

use crate::instance::InstanceKey;
use crate::manifest::PluginId;

/// A named place in the app where plugins put UI. A slot can appear many times
/// on the page, each time with its own context.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    EnumString,
    AsRefStr,
    Display,
)]
pub enum Slot {
    /// The edge of the session's message box. One instance.
    #[serde(rename = "composer")]
    #[strum(serialize = "composer")]
    Composer,
    /// A session's row in the session list. Its nodes cannot be interactive,
    /// since the whole row is a button.
    #[serde(rename = "session.row")]
    #[strum(serialize = "session.row")]
    SessionRow,
    /// The open session's header.
    #[serde(rename = "session.header")]
    #[strum(serialize = "session.header")]
    SessionHeader,
}

impl Slot {
    /// Whether nodes stand on the slot's edge, placed by `align`, `offset` and
    /// `sink`, rather than in normal flow.
    pub fn is_edge(self) -> bool {
        matches!(self, Slot::Composer)
    }

    /// Whether buttons, toggles and inputs may appear in the slot.
    pub fn is_interactive(self) -> bool {
        !matches!(self, Slot::SessionRow)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Start,
    Center,
    #[default]
    End,
}

/// How strongly text is drawn, in the app's own colors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    #[default]
    Normal,
    Dim,
    Accent,
}

/// Names a function the plugin passed in a tree, valid for one render of one
/// instance. The page hands it back with an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CallbackId(u64);

impl From<u64> for CallbackId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl From<CallbackId> for f64 {
    /// A script number. Ids count up from one, far below where a float loses digits.
    fn from(id: CallbackId) -> Self {
        id.0 as f64
    }
}

/// A declarative UI node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Node {
    Image {
        /// A `data:` URL once published.
        src: String,
        width: u16,
        height: u16,
        pixelated: bool,
    },
    Text {
        text: String,
        tone: Tone,
    },
    Row {
        gap: u16,
        children: Vec<Node>,
    },
    Button {
        /// The accessible name. It is also the content when there are no children.
        label: String,
        children: Vec<Node>,
        on_press: CallbackId,
    },
    Toggle {
        label: String,
        value: bool,
        on_change: CallbackId,
    },
    Input {
        label: String,
        value: String,
        placeholder: String,
        on_change: Option<CallbackId>,
        on_submit: Option<CallbackId>,
    },
}

impl Node {
    /// Whether the node or one below it takes input.
    pub(crate) fn is_interactive(&self) -> bool {
        match self {
            Node::Image { .. } | Node::Text { .. } => false,
            Node::Row { children, .. } => children.iter().any(Node::is_interactive),
            Node::Button { .. } | Node::Toggle { .. } | Node::Input { .. } => true,
        }
    }
}

/// Where a placement sits in its slot. Only edge slots place nodes by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Position {
    /// On the slot's edge: `align` picks the side, `offset` the inset from it,
    /// `sink` how far it drops below the edge.
    Edge {
        align: Align,
        offset: u16,
        sink: u16,
    },
    /// In the flow of the page, where the slot is.
    Inline,
}

/// One node a plugin put in one instance of a slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Placement {
    pub plugin: PluginId,
    pub slot: Slot,
    /// The instance of the slot the page reported.
    pub instance: InstanceKey,
    pub position: Position,
    pub node: Node,
}

/// What a plugin's render returns for one placement and instance, before images
/// are resolved.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPlacement {
    pub instance: InstanceKey,
    pub place: RawPlace,
    pub node: RawNode,
}

/// The options of `beep.place`. Each slot takes its own.
#[derive(Debug, Deserialize)]
#[serde(tag = "slot", deny_unknown_fields)]
pub(crate) enum RawPlace {
    #[serde(rename = "composer")]
    Composer {
        #[serde(default)]
        align: Align,
        #[serde(default)]
        offset: u16,
        #[serde(default)]
        sink: u16,
    },
    #[serde(rename = "session.row")]
    SessionRow {},
    #[serde(rename = "session.header")]
    SessionHeader {},
}

impl RawPlace {
    pub(crate) fn slot(&self) -> Slot {
        match self {
            RawPlace::Composer { .. } => Slot::Composer,
            RawPlace::SessionRow {} => Slot::SessionRow,
            RawPlace::SessionHeader {} => Slot::SessionHeader,
        }
    }

    pub(crate) fn position(&self) -> Position {
        match *self {
            RawPlace::Composer {
                align,
                offset,
                sink,
            } => Position::Edge {
                align,
                offset,
                sink,
            },
            RawPlace::SessionRow {} | RawPlace::SessionHeader {} => Position::Inline,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum RawNode {
    Image {
        /// Path relative to the plugin folder.
        src: String,
        width: u16,
        height: u16,
        #[serde(default)]
        pixelated: bool,
    },
    Text {
        text: String,
        #[serde(default)]
        tone: Tone,
    },
    Row {
        #[serde(default)]
        gap: u16,
        children: Vec<RawNode>,
    },
    Button {
        label: String,
        #[serde(default)]
        children: Vec<RawNode>,
        on_press: CallbackId,
    },
    Toggle {
        label: String,
        value: bool,
        on_change: CallbackId,
    },
    Input {
        label: String,
        #[serde(default)]
        value: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default)]
        on_change: Option<CallbackId>,
        #[serde(default)]
        on_submit: Option<CallbackId>,
    },
}
