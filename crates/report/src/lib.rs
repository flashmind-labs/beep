//! Problem reports: the period a report covers, secret redaction and
//! collecting each machine's share.

mod collect;
mod period;
mod redact;

pub use collect::{ASK_LIMIT, Answer, Share, Target, collect};
pub use period::Period;
pub use redact::{RedactError, redact};
