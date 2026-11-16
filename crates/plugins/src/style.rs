use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use strum::{AsRefStr, Display, EnumString, IntoStaticStr};
use thiserror::Error;

/// A style token a plugin may set: the custom properties a skin sets in
/// `apps/desktop/dist/skins.css`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    AsRefStr,
    Display,
    EnumString,
    IntoStaticStr,
    Serialize,
    Deserialize,
)]
#[serde(try_from = "String", into = "&'static str")]
pub enum StyleToken {
    #[strum(serialize = "--ink")]
    Ink,
    #[strum(serialize = "--surface")]
    Surface,
    #[strum(serialize = "--elev")]
    Elev,
    #[strum(serialize = "--deep")]
    Deep,
    #[strum(serialize = "--border")]
    Border,
    #[strum(serialize = "--border-hi")]
    BorderHi,
    #[strum(serialize = "--fg")]
    Fg,
    #[strum(serialize = "--muted")]
    Muted,
    #[strum(serialize = "--dim")]
    Dim,
    #[strum(serialize = "--accent")]
    Accent,
    #[strum(serialize = "--on-accent")]
    OnAccent,
    #[strum(serialize = "--hot")]
    Hot,
    #[strum(serialize = "--ok")]
    Ok,
    #[strum(serialize = "--err")]
    Err,
    #[strum(serialize = "--warn")]
    Warn,
    #[strum(serialize = "--link")]
    Link,
    #[strum(serialize = "--shade")]
    Shade,
    #[strum(serialize = "--body")]
    Body,
    #[strum(serialize = "--display")]
    Display,
    #[strum(serialize = "--mono")]
    Mono,
    #[strum(serialize = "--radius")]
    Radius,
    #[strum(serialize = "--radius-sm")]
    RadiusSm,
    #[strum(serialize = "--label-case")]
    LabelCase,
    #[strum(serialize = "--label-track")]
    LabelTrack,
    #[strum(serialize = "--label-size")]
    LabelSize,
    #[strum(serialize = "--ctl-h")]
    CtlH,
    #[strum(serialize = "--glass")]
    Glass,
    #[strum(serialize = "--glass-solid")]
    GlassSolid,
    #[strum(serialize = "--paintings")]
    Paintings,
}

#[derive(Debug, Error)]
#[error("unknown style token {0:?}: use one of the tokens in the plugin guide")]
pub struct UnknownStyleToken(String);

impl TryFrom<String> for StyleToken {
    type Error = UnknownStyleToken;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        text.parse().map_err(|_| UnknownStyleToken(text))
    }
}

/// The longest value a token may take, in bytes.
pub const MAX_STYLE_VALUE: usize = 200;

/// What a value may never contain: anything that loads a file or a page, such
/// as `url(` in `--glass`, or that ends the declaration it is set in.
const REFUSED: &[&str] = &["url(", "src(", "image", "@", "\\", ";", "{", "}", "<", ">"];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum InvalidStyleValue {
    #[error("style value is empty or longer than {MAX_STYLE_VALUE} bytes")]
    Length,
    #[error("style value {0:?} may not load files or contain {1:?}")]
    Refused(String, &'static str),
}

/// A token's value, as CSS text that loads nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StyleValue(String);

impl TryFrom<String> for StyleValue {
    type Error = InvalidStyleValue;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        if text.trim().is_empty() || text.len() > MAX_STYLE_VALUE {
            return Err(InvalidStyleValue::Length);
        }

        let lower = text.to_ascii_lowercase();
        match REFUSED.iter().find(|part| lower.contains(*part)) {
            Some(part) => Err(InvalidStyleValue::Refused(text, part)),
            None => Ok(Self(text)),
        }
    }
}

impl From<StyleValue> for String {
    fn from(value: StyleValue) -> Self {
        value.0
    }
}

impl AsRef<str> for StyleValue {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// The tokens a skin plugin sets, from its manifest.
pub type Style = BTreeMap<StyleToken, StyleValue>;

#[cfg(test)]
#[path = "tests/style.rs"]
mod tests;
