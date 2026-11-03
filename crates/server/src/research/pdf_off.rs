//! A server built without the `pdf` feature: research papers cannot be
//! typeset, so asking for one fails.

use crate::store::SourceRow;
use anyhow::{Result, bail};
use chrono::NaiveDate;

/// What goes into the PDF.
#[expect(dead_code, reason = "only the typesetting reads it")]
pub struct Paper {
    pub topic: String,
    pub draft: String,
    pub sources: Vec<SourceRow>,
    pub date: NaiveDate,
}

/// A typeset paper.
pub struct Pdf {
    pub name: String,
    pub data: Vec<u8>,
}

pub async fn render(_paper: Paper) -> Result<Pdf> {
    bail!("this server was built without PDF support")
}
