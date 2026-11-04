//! A linked machine as a project [`Shell`].

use crate::nodes::Online;
use anyhow::Result;
use async_trait::async_trait;
use beep_projects::{
    CommandOutput, MachineShell, Shell, ShellError, ShellOutput, imports::clone::tail,
};
use secrecy::{ExposeSecret, SecretString};
use std::time::Duration;

/// Output kept while a script runs, before the end of it is cut down.
const MAX_SCRIPT_OUTPUT: usize = 128 * 1024;
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

#[async_trait]
impl MachineShell for Online {
    async fn run(&self, script: &SecretString, limit: Duration) -> Result<ShellOutput, ShellError> {
        let mut output = String::new();
        let mut on_output = |text: &str| {
            output.push_str(text);
            if output.len() > MAX_SCRIPT_OUTPUT {
                output = tail(&output);
            }
        };
        let exited = self
            .exec(script.expose_secret(), limit, &mut on_output)
            .await
            .map_err(ShellError)?;
        Ok(ShellOutput {
            exit_code: exited.exit_code,
            timed_out: exited.timed_out,
            output,
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
