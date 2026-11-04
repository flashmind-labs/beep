//! A linked machine as a project [`Shell`].

use crate::nodes::Online;
use anyhow::Result;
use async_trait::async_trait;
use beep_projects::{CommandOutput, Shell};
use std::time::Duration;

/// Most output a command keeps.
const MAX_OUTPUT: usize = 64 * 1024;

#[async_trait]
impl Shell for Online {
    async fn run(&self, command: &str, limit: Duration) -> Result<CommandOutput> {
        let mut out = String::new();
        let exited = self
            .exec(command, limit, &mut |text| push_capped(&mut out, text))
            .await?;
        Ok(CommandOutput {
            code: exited.exit_code,
            out,
        })
    }
}

fn push_capped(out: &mut String, text: &str) {
    let room = MAX_OUTPUT.saturating_sub(out.len());
    let mut end = text.len().min(room);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    out.push_str(&text[..end]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_capped_on_a_char_boundary() {
        let mut out = "a".repeat(MAX_OUTPUT - 1);
        push_capped(&mut out, "éé");
        assert_eq!(out.len(), MAX_OUTPUT - 1);
        push_capped(&mut out, "b");
        push_capped(&mut out, "c");
        assert_eq!(out.len(), MAX_OUTPUT);
    }
}
