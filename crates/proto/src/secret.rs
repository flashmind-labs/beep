//! Text that must not show up in logs, such as a token on its way to the
//! server or to a node.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A secret as it crosses the wire. It encodes as plain text, but its
/// `Debug` prints nothing of it, so a stray `{:?}` on a message that holds
/// one cannot leak it.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretText(String);

impl SecretText {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The secret itself. Call it only where the value is used.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretText(hidden)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_secret() {
        let secret = SecretText::new("ghp_example1234");
        assert!(!format!("{secret:?}").contains("1234"));
        let body = rmp_serde::to_vec(&secret).unwrap();
        assert_eq!(
            rmp_serde::from_slice::<String>(&body).unwrap(),
            "ghp_example1234"
        );
    }
}
