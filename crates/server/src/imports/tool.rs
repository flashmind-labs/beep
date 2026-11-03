//! `import_project`: clones a repository onto a user's machine and lists it, like the import sheet.

use super::{
    ImportError,
    service::{self, Import},
};
use crate::tools::machines::{Machines, find, in_parameter};
use async_trait::async_trait;
use flashmind::types::tool::{Tool, ToolContext, ToolResult};
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) const NAME: &str = "import_project";

pub(crate) struct ImportProjectTool {
    pub(crate) machines: Machines,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    repo: String,
    #[serde(default, rename = "in")]
    machine: Option<String>,
    #[serde(default)]
    folder: Option<String>,
}

#[async_trait]
impl Tool for ImportProjectTool {
    fn name(&self) -> &str {
        NAME
    }
    fn description(&self) -> &str {
        "Import a project: clone a git repository onto one of the user's machines and list it \
         under Projects. `repo` is org/repo (GitHub), a GitHub URL, or a URL on a Forgejo or \
         Gitea server the user saved a token for. Without `folder` it goes to \
         <workspace>/projects/<org>/<repo> on their Linux environment, or \
         ~/Projects/<org>/<repo> on a Mac; a checkout already there is reused. With `folder`, \
         a checkout of the repository there is listed in place without cloning, an empty or \
         missing folder gets the clone, and a folder with other files gets it in \
         <folder>/<repo>. Uses the user's tokens from Settings > Credentials. Use it when the \
         user asks to import, add, clone or check out a project or repository, or to list one \
         they already have somewhere."
    }
    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "repo": {
                    "type": "string",
                    "description": "org/repo, such as n0-computer/iroh, or the repository's URL"
                },
                "in": in_parameter("Omit for the user's own Linux environment, or the only machine online."),
                "folder": {
                    "type": "string",
                    "description": "Where it goes on that machine, when the user names a place: an absolute path or one under the home folder, such as ~/Documents/code/iroh. Omit for the default projects folder."
                }
            },
            "required": ["repo"],
            "additionalProperties": false
        })
    }
    fn timeout_secs(&self) -> Option<u64> {
        Some(31 * 60)
    }
    fn humanize(&self, args: &Value) -> String {
        match args.get("repo").and_then(Value::as_str) {
            Some(repo) => format!("Importing {}", repo.trim()),
            None => "Importing a project".to_owned(),
        }
    }
    async fn execute(&self, ctx: ToolContext<'_>) -> anyhow::Result<ToolResult> {
        let args: Args = match serde_json::from_value(ctx.args.clone()) {
            Ok(args) => args,
            Err(e) => return Ok(ToolResult::failure(ctx.tool_call_id, e.to_string())),
        };
        let Machines { app, user } = &self.machines;
        // `in` takes a name; the import takes the machine's id.
        let machine = match args.machine.as_deref().map(str::trim) {
            Some(name) if !name.is_empty() => {
                let nodes = user.nodes().await?;
                match find(&nodes, name) {
                    Some(node) => Some(node.endpoint.clone()),
                    None => {
                        return Ok(ToolResult::failure(
                            ctx.tool_call_id,
                            format!("No machine called {name}."),
                        ));
                    }
                }
            }
            _ => None,
        };
        Ok(
            match service::import(
                app,
                user,
                Import {
                    repo: &args.repo,
                    machine_id: machine.as_deref(),
                    folder: args.folder.as_deref(),
                },
            )
            .await
            {
                Ok(done) => ToolResult::success(
                    ctx.tool_call_id,
                    format!(
                        "{} {} on {} in {}. It shows under Projects.",
                        if done.cloned {
                            "Cloned"
                        } else {
                            "Found an existing checkout of"
                        },
                        done.project,
                        done.machine_name,
                        done.folder
                    ),
                ),
                Err(ImportError::Internal) => {
                    return Err(anyhow::anyhow!("the project import failed"));
                }
                Err(e) => ToolResult::failure(ctx.tool_call_id, e.to_string()),
            },
        )
    }
}
