use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use thiserror::Error;

use crate::asset::AssetRef;
use crate::ident::{PluginId, Slot};
use crate::instance::InstanceKey;

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
    /// shows `rest` unless a motion or a reaction is playing.
    Sprite {
        /// Each distinct frame once. `rest` and steps index into it.
        frames: Vec<AssetRef>,
        rest: u16,
        width: u16,
        height: u16,
        pixelated: bool,
        motions: Vec<Motion>,
        /// Motions that play once when asked, by name.
        reactions: BTreeMap<String, Vec<Step>>,
        /// The reaction to play now. The page plays it once for each new `id`.
        play: Option<Play>,
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

/// A request to play the reaction `name`. A new `id` asks again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Play {
    pub name: String,
    pub id: u32,
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

/// How a placement comes and goes on the page: a short fade or rise, or none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Transition {
    #[default]
    None,
    Fade,
    Rise,
}

/// One node a plugin put in one instance of a slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Placement {
    pub plugin: PluginId,
    pub slot: Slot,
    /// The instance of the slot the page reported.
    pub instance: InstanceKey,
    pub position: Position,
    pub enter: Transition,
    pub leave: Transition,
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
        #[serde(default)]
        enter: Transition,
        #[serde(default)]
        leave: Transition,
    },
    #[serde(rename = "session.row")]
    SessionRow {
        #[serde(default)]
        enter: Transition,
        #[serde(default)]
        leave: Transition,
    },
    #[serde(rename = "session.header")]
    SessionHeader {
        #[serde(default)]
        enter: Transition,
        #[serde(default)]
        leave: Transition,
    },
}

impl RawPlace {
    pub(crate) fn slot(&self) -> Slot {
        match self {
            RawPlace::Composer { .. } => Slot::Composer,
            RawPlace::SessionRow { .. } => Slot::SessionRow,
            RawPlace::SessionHeader { .. } => Slot::SessionHeader,
        }
    }

    pub(crate) fn transitions(&self) -> (Transition, Transition) {
        match *self {
            RawPlace::Composer { enter, leave, .. }
            | RawPlace::SessionRow { enter, leave }
            | RawPlace::SessionHeader { enter, leave } => (enter, leave),
        }
    }

    pub(crate) fn position(&self) -> Position {
        match *self {
            RawPlace::Composer {
                align,
                offset,
                sink,
                ..
            } => Position::Edge {
                align,
                offset,
                sink,
            },
            RawPlace::SessionRow { .. } | RawPlace::SessionHeader { .. } => Position::Inline,
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
        #[serde(default)]
        reactions: BTreeMap<String, RawReaction>,
        #[serde(default)]
        play: Option<Play>,
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

/// A motion that plays once, when the sprite is asked to.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawReaction {
    pub steps: Vec<RawStep>,
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
    #[error("the sprite has no reaction named {0:?}")]
    NoReaction(String),
}

/// Checks the count and durations of one motion's or reaction's steps.
fn check_steps(steps: &[RawStep]) -> Result<(), SpriteError> {
    if steps.is_empty() {
        return Err(SpriteError::NoSteps);
    }
    if steps.len() > MAX_STEPS {
        return Err(SpriteError::TooManySteps(steps.len()));
    }
    if let Some(step) = steps
        .iter()
        .find(|step| !(MIN_STEP_MS..=MAX_STEP_MS).contains(&step.ms))
    {
        return Err(SpriteError::StepDuration(step.ms));
    }

    Ok(())
}

impl RawMotion {
    /// Checks the counts, durations and range, without reading any file.
    pub(crate) fn check(&self) -> Result<(), SpriteError> {
        check_steps(&self.steps)?;
        if let RawRepeat::Every(RawRange { min, max }) = self.repeat {
            let inside = |ms| (MIN_EVERY_MS..=MAX_EVERY_MS).contains(&ms);
            if !inside(min) || !inside(max) || min > max {
                return Err(SpriteError::Range { min, max });
            }
        }

        Ok(())
    }
}

/// Checks the motions and reactions of one sprite together, and that `play` names a reaction.
pub(crate) fn check_motions(
    motions: &[RawMotion],
    reactions: &BTreeMap<String, RawReaction>,
    play: Option<&Play>,
) -> Result<(), SpriteError> {
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

    if reactions.len() > MAX_MOTIONS {
        return Err(SpriteError::TooManyMotions(reactions.len()));
    }
    reactions
        .values()
        .try_for_each(|reaction| check_steps(&reaction.steps))?;

    match play {
        Some(Play { name, .. }) if !reactions.contains_key(name) => {
            Err(SpriteError::NoReaction(name.clone()))
        }
        _ => Ok(()),
    }
}
