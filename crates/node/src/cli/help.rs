//! The grouped `beep-node --help`. clap has no subcommand groups, so the
//! listing is rendered here and the help template leaves out clap's own.
//! Each description is read from the clap model: the first paragraph of a
//! command's doc comment, the rest being its long help.

use clap::{Command, CommandFactory};

use crate::Cli;

/// The groups and the commands under each, in display order. A command
/// may be named with its parent, as `mesh join`.
const GROUPS: &[(&str, &[&str])] = &[
    ("Mesh", &["mesh join", "mesh devices", "mesh leave"]),
    ("Agents", &["sessions", "hooks"]),
    ("Service", &["run", "service", "update"]),
    ("Diagnostics", &["status", "logs", "report", "env", "id"]),
    ("Other", &["version"]),
];

fn template(cmd: &Command) -> String {
    let args = if cmd.get_positionals().next().is_some() {
        "\n\nArguments:\n{positionals}"
    } else {
        ""
    };
    format!(
        "{{about-with-newline}}\n\
         {{usage-heading}} {{usage}}{{after-help}}{args}\n\
         \n\
         Options:\n\
         {{options}}\n\
         \n\
         Run `beep-node help <command>` for the full description of one command."
    )
}

/// The command line, with the grouped listing in place of clap's.
pub(crate) fn command() -> Command {
    let cmd = Cli::command();
    let listing = render(&cmd);
    let template = template(&cmd);
    cmd.help_template(template).after_help(listing)
}

/// The command a grouped name stands for.
fn find<'a>(cmd: &'a Command, name: &str) -> Option<&'a Command> {
    name.split(' ')
        .try_fold(cmd, |parent, part| parent.find_subcommand(part))
}

fn render(cmd: &Command) -> String {
    let width = GROUPS
        .iter()
        .flat_map(|(_, names)| names.iter())
        .map(|name| name.len())
        .max()
        .unwrap_or(0);
    let mut blocks = Vec::new();

    for (title, names) in GROUPS {
        let mut block = String::from(*title);

        for name in *names {
            let Some(sub) = find(cmd, name) else {
                continue;
            };
            let about = sub.get_about().map(|a| a.to_string()).unwrap_or_default();
            block.push_str(&format!("\n  {name:width$}  {about}"));
            let aliases: Vec<&str> = sub.get_visible_aliases().collect();

            match aliases.len() {
                0 => {}
                1 => block.push_str(&format!(" [alias: {}]", aliases[0])),
                _ => block.push_str(&format!(" [aliases: {}]", aliases.join(", "))),
            }
        }

        blocks.push(block);
    }

    blocks.join("\n\n")
}

#[cfg(test)]
#[path = "../tests/cli_help.rs"]
mod tests;
