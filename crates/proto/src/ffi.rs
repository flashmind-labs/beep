//! What Kotlin and Swift need for the foreign types on the wire.

use flashmind_types::ReasoningLevel;
use uuid::Uuid;

// Kotlin and Swift see ids as their text.
uniffi::custom_type!(Uuid, String, {
    remote,
    try_lift: |text| Ok(Uuid::parse_str(&text)?),
    lower: |id| id.to_string(),
});

/// How long the model reasons before it answers. `Off` sends no reasoning
/// setting, so the model uses its own default.
#[uniffi::remote(Enum)]
pub enum ReasoningLevel {
    Off,
    Low,
    Medium,
    High,
}
