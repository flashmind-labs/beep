//! A linked machine as a project [`Shell`].

use crate::nodes::Online;
use anyhow::Result;
use async_trait::async_trait;
use beep_projects::{Clip, Shell, ShellOutput};
use std::time::Duration;

#[async_trait]
impl Shell for Online {
    async fn run(&self, script: &str, limit: Duration) -> Result<ShellOutput> {
        let mut clip = Clip::default();
        let exited = self
            .exec(script, limit, &mut |text| clip.push(text))
            .await?;
        Ok(ShellOutput {
            exit_code: exited.exit_code,
            timed_out: exited.timed_out,
            output: clip.finish(),
        })
    }
}
