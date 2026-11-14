use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString};
use thiserror::Error;

use crate::asset::AssetRef;
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
        /// The page fetches the bytes once per hash.
        asset: AssetRef,
        width: u16,
        height: u16,
        pixelated: bool,
    },
    /// Frames the page plays by itself, with no wakeups of the plugin. It
    /// shows `rest` unless a motion is playing.
    Sprite {
        /// Each distinct frame once. `rest` and steps index into it.
        frames: Vec<AssetRef>,
        rest: u16,
        width: u16,
        height: u16,
        pixelated: bool,
        motions: Vec<Motion>,
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
            Node::Image { .. } | Node::Sprite { .. } | Node::Text { .. } => false,
            Node::Row { children, .. } => children.iter().any(Node::is_interactive),
            Node::Button { .. } | Node::Toggle { .. } | Node::Input { .. } => true,
        }
    }
}

/// Frames shown in turn: each step shows its frame for `ms`, then the sprite
/// goes back to its rest frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Motion {
    pub steps: Vec<Step>,
    pub repeat: Repeat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Step {
    /// Index into the sprite's frames.
    pub frame: u16,
    pub ms: u32,
}

/// When a motion plays, as the page reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Repeat {
    /// Again and again with no rest. The only motion of its sprite.
    Loop,
    /// Once, when the sprite appears.
    Once,
    /// Once after a random delay in the range, and again after each play.
    Every { min_ms: u32, max_ms: u32 },
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
    Sprite {
        /// Path of the frame shown when no motion plays.
        rest: String,
        width: u16,
        height: u16,
        #[serde(default)]
        pixelated: bool,
        #[serde(default)]
        motions: Vec<RawMotion>,
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawMotion {
    pub steps: Vec<RawStep>,
    #[serde(default)]
    pub repeat: RawRepeat,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawStep {
    /// Path of the frame, relative to the plugin folder.
    pub frame: String,
    pub ms: u32,
}

/// `"loop"`, `"once"` (the default) or `{ every: { min, max } }`, in ms.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RawRepeat {
    Loop,
    #[default]
    Once,
    Every(RawRange),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRange {
    pub min: u32,
    pub max: u32,
}

/// The most motions, steps in one, and distinct frames of one sprite.
pub(crate) const MAX_MOTIONS: usize = 8;
pub(crate) const MAX_STEPS: usize = 64;
pub(crate) const MAX_FRAMES: usize = 32;
/// The shortest and longest a step lasts, in ms.
pub(crate) const MIN_STEP_MS: u32 = 16;
pub(crate) const MAX_STEP_MS: u32 = 60_000;
/// The shortest and longest delay of a repeating motion, in ms.
pub(crate) const MIN_EVERY_MS: u32 = 100;
pub(crate) const MAX_EVERY_MS: u32 = 3_600_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum SpriteError {
    #[error("a sprite has {0} motions, the limit is {MAX_MOTIONS}")]
    TooManyMotions(usize),
    #[error("a motion needs at least one step")]
    NoSteps,
    #[error("a motion has {0} steps, the limit is {MAX_STEPS}")]
    TooManySteps(usize),
    #[error("a sprite has {0} frames, the limit is {MAX_FRAMES}")]
    TooManyFrames(usize),
    #[error("a step of {0} ms is outside {MIN_STEP_MS} to {MAX_STEP_MS}")]
    StepDuration(u32),
    #[error(
        "an every range of {min} to {max} ms must lie within {MIN_EVERY_MS} to {MAX_EVERY_MS} with min at most max"
    )]
    Range { min: u32, max: u32 },
    #[error("a looping motion must be the only motion")]
    LoopAlone,
}

impl RawMotion {
    /// Checks the counts, durations and range, without reading any file.
    pub(crate) fn check(&self) -> Result<(), SpriteError> {
        if self.steps.is_empty() {
            return Err(SpriteError::NoSteps);
        }
        if self.steps.len() > MAX_STEPS {
            return Err(SpriteError::TooManySteps(self.steps.len()));
        }
        if let Some(step) = self
            .steps
            .iter()
            .find(|step| !(MIN_STEP_MS..=MAX_STEP_MS).contains(&step.ms))
        {
            return Err(SpriteError::StepDuration(step.ms));
        }
        if let RawRepeat::Every(RawRange { min, max }) = self.repeat {
            let inside = |ms| (MIN_EVERY_MS..=MAX_EVERY_MS).contains(&ms);
            if !inside(min) || !inside(max) || min > max {
                return Err(SpriteError::Range { min, max });
            }
        }

        Ok(())
    }
}

/// Checks the motions of one sprite together.
pub(crate) fn check_motions(motions: &[RawMotion]) -> Result<(), SpriteError> {
    if motions.len() > MAX_MOTIONS {
        return Err(SpriteError::TooManyMotions(motions.len()));
    }
    motions.iter().try_for_each(RawMotion::check)?;

    let loops = motions
        .iter()
        .any(|motion| matches!(motion.repeat, RawRepeat::Loop));
    if loops && motions.len() > 1 {
        return Err(SpriteError::LoopAlone);
    }

    Ok(())
}
