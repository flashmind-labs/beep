//! `project_setup`: the checkout, machine and roles a project works from.

use super::{
    super::{
        fleet::{self, Machine},
        gate::{Asks, Gate},
    },
    CHECKOUT_SLUG, ProjectTools, SETUP, card, field, machine_list,
};
use crate::{
    store::{CheckoutRow, ProjectRow},
    tools::machines::command_name,
};
use anyhow::Result;
use async_trait::async_trait;
use beep_projects::{
    commands::{HostCommand, exclude_project_dirs, parse_setup, parse_visibility, read_setup},
    issues::{HOST_LIMIT, run_host},
    model::{IssueHost, MachinePath},
};
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fmt::Write, sync::Arc};

pub(super) struct SetupTool {
    pub(super) tools: Arc<ProjectTools>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SetupAction {
    Checkout,
    Home,
    Pin,
    Show,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SetupArgs {
    action: SetupAction,
    #[serde(default, rename = "in")]
    machine: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[async_trait]
impl Tool for SetupTool {
    fn name(&self) -> &str {
        SETUP
    }

    fn description(&self) -> &str {
        "Set up where the project lives. `checkout` adds the clone at `path` on the machine `in` \
         names: it must be in a folder that machine shares. It reads the remote, the beep.issues \
         and beep.prs git settings and the default branch, and the first checkout becomes the \
         home one, where outputs are copied. `home` makes another checkout the home one, `pin` \
         makes workers start on the machine `in` names first, and `show` lists the setup."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["checkout", "home", "pin", "show"]},
                "in": {"type": "string", "description": "The machine, by its name in the machines list."},
                "path": {"type": "string", "description": "The checkout's absolute path on that machine, for checkout and home."}
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: ToolContext<'_>) -> Result<ToolResult> {
        let id = ctx.tool_call_id;
        let args: SetupArgs = match ctx.parse_args(SETUP) {
            Ok(args) => args,
            Err(error) => return Ok(ToolResult::failure(id, error.to_string())),
        };
        if let SetupAction::Checkout = args.action {
            let checkout_card = || {
                let text = |value: &Option<String>| value.clone().unwrap_or_default();
                card(
                    "Add a checkout".to_owned(),
                    vec![
                        field("Machine", text(&args.machine)),
                        field("Path", text(&args.path)),
                    ],
                )
            };
            let gate = self
                .tools
                .gate
                .check(id, CHECKOUT_SLUG, Asks::ByLevel, checkout_card);
            match gate {
                Gate::Run => {}
                Gate::Refuse(reason) => return Ok(ToolResult::success(id, reason)),
                Gate::Park(result) => return Ok(result),
            }
        }
        let done = match args.action {
            SetupAction::Show => self.show().await?,
            action => self.change(action, &args).await?,
        };
        Ok(match done {
            Ok(text) => ToolResult::success(id, text),
            Err(text) => ToolResult::failure(id, text),
        })
    }

    fn humanize(&self, _args: &Value) -> String {
        "Setting up the project".to_owned()
    }
}

impl SetupTool {
    pub(super) async fn show(&self) -> Result<Result<String, String>> {
        let tools = &self.tools;
        let project = tools.project().await?;
        let machines = tools.fleet.machines().await?;
        let mut text = format!("{}\n", project.name);
        let role =
            |host: Option<&IssueHost>| host.map_or("kept in Beep".to_owned(), IssueHost::to_string);
        let _ = writeln!(text, "Issues: {}", role(project.roles.issues.as_ref()));
        let _ = writeln!(text, "Pull requests: {}", role(project.roles.prs.as_ref()));
        let visibility: &'static str = project.visibility.into();
        let _ = writeln!(text, "Visibility: {visibility}");
        let _ = writeln!(text, "Default branch: {}", project.default_branch);
        for checkout in tools.user.checkouts(project.id).await? {
            let home = if checkout.home { " (home)" } else { "" };
            let _ = writeln!(
                text,
                "Checkout: {} on {}{home}",
                checkout.path,
                fleet::name_of(&machines, &checkout.machine)
            );
        }
        if let Some(pin) = &project.pin {
            let _ = writeln!(text, "Pinned to: {}", fleet::name_of(&machines, pin));
        }
        Ok(Ok(text))
    }

    pub(super) async fn change(
        &self,
        action: SetupAction,
        args: &SetupArgs,
    ) -> Result<Result<String, String>> {
        let tools = &self.tools;
        let project = tools.project().await?;
        let machines = tools.fleet.machines().await?;
        let Some(name) = args.machine.as_deref() else {
            return Ok(Err(format!(
                "Name the machine with `in`. {}",
                machine_list(&machines)
            )));
        };
        let Some(machine) = fleet::find(&machines, name) else {
            return Ok(Err(format!(
                "No machine called {name}. {}",
                machine_list(&machines)
            )));
        };
        if let SetupAction::Pin = action {
            tools.user.set_pin(project.id, Some(&machine.id)).await?;
            return Ok(Ok(format!(
                "Workers start on {} first.",
                command_name(&machine.name)
            )));
        }
        let path = match args.path.as_deref().map(MachinePath::parse) {
            Some(Ok(path)) => path,
            Some(Err(error)) => return Ok(Err(format!("Not a usable path: {error}."))),
            None => return Ok(Err("Give the checkout's `path`.".to_owned())),
        };
        if let SetupAction::Home = action {
            return Ok(
                match tools.user.set_home(project.id, &machine.id, &path).await {
                    Ok(()) => Ok(format!(
                        "{path} on {} is the home checkout.",
                        command_name(&machine.name)
                    )),
                    Err(error) => Err(format!("{error:#}.")),
                },
            );
        }
        self.checkout(&project, machine, &path).await
    }

    pub(super) async fn checkout(
        &self,
        project: &ProjectRow,
        machine: &Machine,
        path: &MachinePath,
    ) -> Result<Result<String, String>> {
        let tools = &self.tools;
        let name = command_name(&machine.name);
        match fleet::shared(tools.fleet.as_ref(), &machine.id, path).await {
            Ok(true) => {}
            Ok(false) => {
                return Ok(Err(format!(
                    "{path} is not in a folder {name} shares, so workers could not copy files \
                     there. Its owner lists shared folders as `files = [\"/path\"]` in \
                     beep-node's node.toml and restarts it."
                )));
            }
            Err(error) => {
                return Ok(Err(format!(
                    "Could not read the folders {name} shares: {error:#}."
                )));
            }
        }
        let Some(shell) = tools.fleet.shell(&machine.id) else {
            return Ok(Err(format!("{name} is offline or offers no shell.")));
        };
        let out = shell.run(&read_setup(path), HOST_LIMIT).await?;
        let setup = match parse_setup(&out.output, &project.name) {
            Ok(setup) => setup,
            Err(error) => return Ok(Err(error.to_string())),
        };
        let visibility = match setup.roles.prs.as_ref().or(setup.roles.issues.as_ref()) {
            Some(host) => {
                let command = HostCommand::Visibility.render(host, &project.name);
                let out = shell.run(&command, HOST_LIMIT).await?;
                parse_visibility(host, &out.output)
            }
            None => project.visibility,
        };
        let excluded = shell.run(&exclude_project_dirs(path), HOST_LIMIT).await?;
        if excluded.exit_code != Some(0) {
            return Ok(Err(format!(
                "Could not keep .worktrees/ and .beep/ out of git there: {}",
                excluded.output.trim()
            )));
        }
        if let Some(host) = &setup.roles.issues
            && let Err(error) = run_host(
                shell.as_ref(),
                host,
                &project.name,
                HostCommand::EnsureLabels,
            )
            .await
        {
            return Ok(Err(format!(
                "Could not make the beep labels on {}: {error:#}. Check that the CLI is \
                 signed in there.",
                host
            )));
        }
        let checkouts = tools.user.checkouts(project.id).await?;
        let home = checkouts.iter().all(|checkout| !checkout.home)
            || checkouts.iter().any(|checkout| {
                checkout.home && checkout.machine == machine.id && &checkout.path == path
            });
        tools
            .user
            .add_checkout(
                project.id,
                &CheckoutRow {
                    machine: machine.id,
                    path: path.clone(),
                    home,
                },
            )
            .await?;
        tools
            .user
            .set_project_setup(project.id, &setup.roles, visibility, &setup.default_branch)
            .await?;
        let role =
            |host: Option<&IssueHost>| host.map_or("kept in Beep".to_owned(), IssueHost::to_string);
        let visibility: &'static str = visibility.into();
        Ok(Ok(format!(
            "Added {path} on {name}{}. Issues: {}. Pull requests: {}. Default branch: {}. The \
             repository is {visibility}.",
            if home { " as the home checkout" } else { "" },
            role(setup.roles.issues.as_ref()),
            role(setup.roles.prs.as_ref()),
            setup.default_branch,
        )))
    }
}
