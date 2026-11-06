//! Problem reports: the period a report covers and secret redaction.

mod period;
mod redact;

pub use period::Period;
pub use redact::{RedactError, redact};
