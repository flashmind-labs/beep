//! When a project tool may act: the Project app's level, a saved "always" and the call just
//! allowed. Unlike `permissions::guard`, each tool decides what its call asks for.

use super::tools::{ProjectTools, leaked};
use crate::{
    apps::permissions::{ApprovalPayload, PROJECT_APP, Rules, ToolSlug},
    protocol::{Approval, ApprovalField, PermissionLevel},
};
use async_trait::async_trait;
use beep_projects::model::Visibility;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde_json::Value;
use std::{mem, sync::Arc};

/// What a call asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Asks {
    /// Runs under Allow or a saved Always, else asks first.
    ByLevel,
    /// Asks first every time.
    Always,
}

/// What the gate says about one call.
#[derive(Debug)]
pub(crate) enum Gate {
    Run,
    /// Not done, with the result the model reads.
    Refuse(String),
    /// Stops the run on this card until the user answers.
    Park(ToolResult),
}

/// The project's rules for one turn.
pub(crate) struct ProjectGate {
    rules: Rules,
    /// The call the user just allowed once.
    approved: Option<String>,
    visibility: Visibility,
}

impl ProjectGate {
    pub(crate) fn new(rules: Rules, approved: Option<String>, visibility: Visibility) -> Self {
        Self {
            rules,
            approved,
            visibility,
        }
    }

    fn level(&self) -> PermissionLevel {
        self.rules.level(PROJECT_APP.slug)
    }

    pub(crate) fn check(
        &self,
        call_id: &str,
        tool: &str,
        asks: Asks,
        card: impl FnOnce() -> Approval,
    ) -> Gate {
        let level = self.level();
        if level == (PermissionLevel::ReadOnly {}) {
            return Gate::Refuse(
                "Not done: the user set Project to read only. Tell them, and that they can \
                 change it under Apps in Beep's menu."
                    .to_owned(),
            );
        }
        let approved = self.approved.as_deref() == Some(call_id);
        let runs = approved
            || match asks {
                Asks::Always => false,
                Asks::ByLevel => {
                    level == (PermissionLevel::Allow {})
                        || self
                            .rules
                            .allows(PROJECT_APP.slug, ToolSlug::new(tool).as_str())
                }
            };
        if runs {
            return Gate::Run;
        }
        Gate::Park(ToolResult::interrupt(
            call_id,
            Arc::new(ApprovalPayload { approval: card() }),
        ))
    }

    /// Whether a worker may push its branch and open a PR unasked: Allow, on a private repo only.
    pub(crate) fn outside_allowed(&self) -> bool {
        self.level() == (PermissionLevel::Allow {}) && self.visibility == Visibility::Private
    }
}

/// Code host CLIs. They can merge, close or publish, so a command running one always asks.
const HOST_CLIS: [&str; 3] = ["gh", "tea", "teacli"];

/// Whether `command` runs a code host's CLI anywhere, quoted text included.
pub(crate) fn runs_host_cli(command: &str) -> bool {
    command
        .split(|c: char| c.is_whitespace() || ";|&()`$<>{}'\"=".contains(c))
        .filter(|word| !word.is_empty())
        .any(|word| HOST_CLIS.contains(&word.rsplit('/').next().unwrap_or(word)))
}

/// The rule slug for `exec`; host CLI commands and pushes to `default_branch` get none.
pub(crate) fn exec_slug(command: &str, default_branch: &str) -> Option<&'static str> {
    (exec_asks(command, default_branch) == Asks::ByLevel).then_some(EXEC_SLUG)
}

/// What `exec` of `command` asks for.
fn exec_asks(command: &str, default_branch: &str) -> Asks {
    if runs_host_cli(command) || pushes_default(command, default_branch) {
        Asks::Always
    } else {
        Asks::ByLevel
    }
}

/// Whether `command` has a `git push` that may update `default_branch` (named, `--all`, or no
/// branch at all). Quoted text counts too, as in `sh -c`.
pub(crate) fn pushes_default(command: &str, default_branch: &str) -> bool {
    shell_segments(command)
        .iter()
        .any(|words| push_args(words).is_some_and(|args| push_reaches(args, default_branch)))
}

/// The words of each simple command, split at `;`, `|`, `&`, parens, backquotes and newlines.
fn shell_segments(command: &str) -> Vec<Vec<&str>> {
    let mut segments: Vec<Vec<&str>> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut start = None;
    for (at, c) in command.char_indices() {
        let breaks = c.is_whitespace() || "'\"{}$".contains(c);
        let separates = ";|&()`\n".contains(c);
        if breaks || separates {
            if let Some(from) = start.take() {
                current.push(&command[from..at]);
            }
            if separates {
                segments.push(mem::take(&mut current));
            }
        } else if start.is_none() {
            start = Some(at);
        }
    }
    if let Some(from) = start {
        current.push(&command[from..]);
    }
    segments.push(current);
    segments
}

/// The arguments after `push` when `words` run `git ... push`.
fn push_args<'a>(words: &'a [&'a str]) -> Option<&'a [&'a str]> {
    let git = words
        .iter()
        .position(|word| word.rsplit('/').next() == Some("git"))?;
    let push = git + 1 + words[git + 1..].iter().position(|word| *word == "push")?;
    Some(&words[push + 1..])
}

fn push_reaches(args: &[&str], default_branch: &str) -> bool {
    // Options that take the next word as their value.
    const VALUED: [&str; 4] = ["-o", "--push-option", "--repo", "--receive-pack"];
    let mut positional = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match *arg {
            "--all" | "--mirror" | "--branches" => return true,
            flag if VALUED.contains(&flag) => {
                args.next();
            }
            flag if flag.starts_with('-') => {}
            other => positional.push(other),
        }
    }
    let Some((_, refspecs)) = positional.split_first() else {
        return true;
    };
    refspecs.is_empty()
        || refspecs.iter().any(|refspec| {
            let target = refspec.rsplit(':').next().unwrap_or(refspec);
            let target = target.trim_start_matches('+');
            let target = target.strip_prefix("refs/heads/").unwrap_or(target);
            target == default_branch || target == "HEAD"
        })
}

/// The flags whose value a code host publishes: titles, bodies, comments.
const TEXT_FLAGS: [&str; 10] = [
    "-t",
    "--title",
    "-b",
    "--body",
    "-m",
    "--message",
    "-c",
    "--comment",
    "-d",
    "--description",
];

/// The text a host CLI command would publish, nested `sh -c` included; unsplittable means whole.
fn host_text_args(command: &str) -> Vec<String> {
    let Some(words) = shlex::split(command) else {
        return vec![command.to_owned()];
    };
    let mut texts = Vec::new();
    let mut words = words.into_iter();
    while let Some(word) = words.next() {
        if word.contains(char::is_whitespace) && runs_host_cli(&word) {
            // A nested script, such as the one `sh -c` runs.
            texts.extend(host_text_args(&word));
        } else if TEXT_FLAGS.contains(&word.as_str()) {
            match words.next() {
                // `sh -c` runs a script rather than publishing it.
                Some(value) if word == "-c" && runs_host_cli(&value) => {
                    texts.extend(host_text_args(&value));
                }
                value => texts.extend(value),
            }
        } else if let Some((flag, value)) = word.split_once('=')
            && TEXT_FLAGS.contains(&flag)
        {
            texts.push(value.to_owned());
        }
    }
    texts
}

const EXEC_SLUG: &str = "EXEC";
pub(crate) const PROCESS_STOP_SLUG: &str = "PROCESS_STOP";

/// What a gated wrapper does with a call.
async fn gated_run(
    tool: &dyn Tool,
    gate: &ProjectGate,
    ctx: ToolContext<'_>,
    slug: &str,
    asks: Asks,
    card: Approval,
) -> anyhow::Result<ToolResult> {
    match gate.check(ctx.tool_call_id, slug, asks, || card) {
        Gate::Run => tool.execute(ctx).await,
        Gate::Refuse(reason) => Ok(ToolResult::success(ctx.tool_call_id, reason)),
        Gate::Park(result) => Ok(result),
    }
}

fn text_arg(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// `exec` in a project chat: asks unless Project is Allow; host CLI commands always ask.
pub(crate) struct GatedExec {
    pub(crate) exec: Arc<dyn Tool>,
    pub(crate) tools: Arc<ProjectTools>,
}

#[async_trait]
impl Tool for GatedExec {
    fn name(&self) -> &str {
        self.exec.name()
    }

    fn description(&self) -> &str {
        self.exec.description()
    }

    fn parameters(&self) -> Value {
        self.exec.parameters()
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let command = text_arg(&ctx.args, "command");
        let host = runs_host_cli(&command);
        let project = self.tools.project().await?;
        let asks = exec_asks(&command, &project.default_branch);
        if host {
            let public_text = self.tools.public_text().await?;
            let mut hits: Vec<String> = Vec::new();
            for hit in host_text_args(&command)
                .iter()
                .flat_map(|text| public_text.hits(text))
            {
                if !hits.contains(&hit) {
                    hits.push(hit);
                }
            }
            if !hits.is_empty() {
                return Ok(ToolResult::failure(ctx.tool_call_id, leaked(&hits)));
            }
        }
        let mut fields = vec![ApprovalField {
            label: "Command".into(),
            value: command,
        }];
        let machine = text_arg(&ctx.args, "in");
        if !machine.is_empty() {
            fields.push(ApprovalField {
                label: "Machine".into(),
                value: machine,
            });
        }
        let card = Approval {
            app: PROJECT_APP.name.to_owned(),
            action: "Run a command".into(),
            fields,
        };
        gated_run(
            self.exec.as_ref(),
            &self.tools.gate,
            ctx,
            EXEC_SLUG,
            asks,
            card,
        )
        .await
    }

    fn timeout_secs(&self) -> Option<u64> {
        self.exec.timeout_secs()
    }

    fn max_output_bytes(&self) -> usize {
        self.exec.max_output_bytes()
    }

    fn max_output_lines(&self) -> usize {
        self.exec.max_output_lines()
    }

    fn humanize(&self, args: &Value) -> String {
        self.exec.humanize(args)
    }

    fn concurrent(&self) -> bool {
        self.exec.concurrent()
    }
}

/// `process` in a project chat: stopping asks unless Project is Allow; listing and reading do not.
pub(crate) struct GatedProcess {
    pub(crate) process: Arc<dyn Tool>,
    pub(crate) gate: Arc<ProjectGate>,
}

#[async_trait]
impl Tool for GatedProcess {
    fn name(&self) -> &str {
        self.process.name()
    }

    fn description(&self) -> &str {
        self.process.description()
    }

    fn parameters(&self) -> Value {
        self.process.parameters()
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        if text_arg(&ctx.args, "action") != "stop" {
            return self.process.execute(ctx).await;
        }
        let mut fields = Vec::new();
        if let Some(id) = ctx.args.get("id").and_then(Value::as_u64) {
            fields.push(ApprovalField {
                label: "Process".into(),
                value: id.to_string(),
            });
        }
        let machine = text_arg(&ctx.args, "in");
        if !machine.is_empty() {
            fields.push(ApprovalField {
                label: "Machine".into(),
                value: machine,
            });
        }
        let card = Approval {
            app: PROJECT_APP.name.to_owned(),
            action: "Stop a background command".into(),
            fields,
        };
        gated_run(
            self.process.as_ref(),
            &self.gate,
            ctx,
            PROCESS_STOP_SLUG,
            Asks::ByLevel,
            card,
        )
        .await
    }

    fn timeout_secs(&self) -> Option<u64> {
        self.process.timeout_secs()
    }

    fn humanize(&self, args: &Value) -> String {
        self.process.humanize(args)
    }

    fn concurrent(&self) -> bool {
        self.process.concurrent()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AppPermissionRow;

    fn rules(level: PermissionLevel, allowed: &[&str]) -> Rules {
        Rules::from_rows(vec![AppPermissionRow {
            app: "project".into(),
            level,
            allowed: allowed.iter().map(|tool| (*tool).to_owned()).collect(),
        }])
    }

    fn card() -> Approval {
        Approval {
            app: "Project".into(),
            action: "Create 1 task".into(),
            fields: Vec::new(),
        }
    }

    fn parks(gate: &ProjectGate, call: &str, asks: Asks) -> bool {
        match gate.check(call, "project_plan", asks, card) {
            Gate::Park(result) => {
                let payload = result.payload().expect("a parked call has a payload");
                payload.as_any().downcast_ref::<ApprovalPayload>().is_some()
            }
            Gate::Run => false,
            Gate::Refuse(reason) => panic!("refused: {reason}"),
        }
    }

    #[test]
    fn gate_rules() {
        let ask = ProjectGate::new(Rules::default(), None, Visibility::Private);
        assert!(parks(&ask, "c1", Asks::ByLevel));
        let approved = ProjectGate::new(Rules::default(), Some("c1".into()), Visibility::Private);
        assert!(!parks(&approved, "c1", Asks::ByLevel));
        assert!(parks(&approved, "c2", Asks::ByLevel));
        assert!(!parks(&approved, "c1", Asks::Always));
        let always = ProjectGate::new(
            rules(PermissionLevel::Ask {}, &["PROJECT_PLAN"]),
            None,
            Visibility::Private,
        );
        assert!(!parks(&always, "c1", Asks::ByLevel));
        assert!(parks(&always, "c1", Asks::Always));
        let allow = ProjectGate::new(
            rules(PermissionLevel::Allow {}, &[]),
            None,
            Visibility::Private,
        );
        assert!(!parks(&allow, "c1", Asks::ByLevel));
        assert!(parks(&allow, "c1", Asks::Always));
        assert!(allow.outside_allowed());
        assert!(!ask.outside_allowed());
        let read_only = ProjectGate::new(
            rules(PermissionLevel::ReadOnly {}, &[]),
            Some("c1".into()),
            Visibility::Private,
        );
        for asks in [Asks::ByLevel, Asks::Always] {
            assert!(matches!(
                read_only.check("c1", "project_plan", asks, card),
                Gate::Refuse(_)
            ));
        }
    }

    #[test]
    fn host_clients_are_found_anywhere_in_a_command() {
        for command in [
            "gh pr merge 5",
            "cd /srv/x && /usr/local/bin/gh issue close 2",
            "echo ok; tea pr list",
            "(teacli issue create -t x)",
        ] {
            assert!(runs_host_cli(command), "{command}");
            assert_eq!(exec_slug(command, "main"), None);
        }
        for command in ["cargo test", "git push -u origin beep/x", "ls ghost tear"] {
            assert!(!runs_host_cli(command), "{command}");
            assert_eq!(exec_slug(command, "main"), Some("EXEC"));
        }
    }

    #[test]
    fn host_text_args_are_the_published_text() {
        assert_eq!(
            host_text_args("cd /srv/x && gh pr create -t 'Fix it' --body=Done -B main"),
            ["Fix it", "Done"]
        );
        assert_eq!(
            host_text_args("sh -c \"tea issue create -t Hi -d 'See notes'\""),
            ["Hi", "See notes"]
        );
        assert_eq!(
            host_text_args("gh issue create -t 'Fix gh login' -b 'In /home/sam'"),
            ["Fix gh login", "In /home/sam"]
        );
        assert_eq!(
            host_text_args("gh pr create -t 'unclosed"),
            ["gh pr create -t 'unclosed"]
        );
    }

    #[test]
    fn public_repos_keep_workers_asking() {
        let allow = ProjectGate::new(
            rules(PermissionLevel::Allow {}, &[]),
            None,
            Visibility::Public,
        );
        assert!(!allow.outside_allowed());
    }
}
