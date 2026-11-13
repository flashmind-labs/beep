use serde::{Deserialize, Serialize};

use crate::manifest::PluginId;

/// A named place in the app where plugins put UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Slot {
    /// The edge of the session's message box.
    Composer,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Start,
    Center,
    #[default]
    End,
}

/// A declarative UI node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
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
    },
    Row {
        gap: u16,
        children: Vec<Node>,
    },
}

/// One node a plugin put in a slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Placement {
    pub plugin: PluginId,
    pub slot: Slot,
    pub align: Align,
    pub offset: u16,
    pub sink: u16,
    pub node: Node,
}

/// What a plugin's `render()` returns for one placement, before images are resolved.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawPlacement {
    pub slot: Slot,
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub offset: u16,
    #[serde(default)]
    pub sink: u16,
    pub node: RawNode,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
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
    },
    Row {
        #[serde(default)]
        gap: u16,
        children: Vec<RawNode>,
    },
}
