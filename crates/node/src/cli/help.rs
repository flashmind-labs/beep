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
    ("Diagnostics", &["status", "logs", "report", "env"]),
    ("Setup", &["setup", "version", "link", "local"]),
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
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// No line of the listing may wrap in an 80 column terminal. The
    /// options below it are clap's to wrap.
    const LIMIT: usize = 80;

    fn grouped() -> impl Iterator<Item = &'static str> {
        GROUPS.iter().flat_map(|(_, names)| names.iter().copied())
    }

    #[test]
    fn every_command_is_grouped_once() {
        let cmd = Cli::command();
        let names: Vec<&str> = grouped().collect();
        let unique: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "a command is grouped twice");
        let tops: HashSet<&str> = names
            .iter()
            .map(|name| name.split(' ').next().unwrap_or(name))
            .collect();
        let missing: Vec<&str> = cmd
            .get_subcommands()
            .filter(|sub| !sub.is_hide_set())
            .map(Command::get_name)
            .filter(|name| !tops.contains(name))
            .collect();
        assert!(missing.is_empty(), "in no group: {missing:?}");
        // A command named by its parent lists every visible child.
        for parent in names.iter().filter_map(|name| name.split_once(' ')) {
            let sub = cmd.find_subcommand(parent.0).expect("a command");
            for child in sub.get_subcommands().filter(|c| !c.is_hide_set()) {
                let full = format!("{} {}", parent.0, child.get_name());
                assert!(unique.contains(full.as_str()), "in no group: {full}");
            }
        }
    }

    #[test]
    fn every_grouped_name_is_a_command() {
        let cmd = Cli::command();
        let dangling: Vec<&str> = grouped().filter(|n| find(&cmd, n).is_none()).collect();
        assert!(dangling.is_empty(), "not commands: {dangling:?}");
    }

    #[test]
    fn no_listing_line_wraps_at_eighty_columns() {
        let help = command().term_width(200).render_help().to_string();
        let long: Vec<&str> = help
            .lines()
            .take_while(|line| *line != "Options:")
            .filter(|line| line.chars().count() > LIMIT)
            .collect();
        assert!(long.is_empty(), "these lines wrap: {long:#?}");
    }

    /// The spellings the installers, the services and older docs use.
    #[test]
    fn the_spellings_in_use_still_parse() {
        for line in [
            vec!["beep-node", "setup", "bk1-abc", "--no-run"],
            vec!["beep-node", "--dir", "/srv/node", "run", "--with-parent"],
            vec!["beep-node", "--log-file", "/srv/node.log", "run"],
            vec!["beep-node", "mesh", "join", "-", "--search", "/srv"],
            vec!["beep-node", "mesh", "devices"],
            vec!["beep-node", "mesh", "peers"],
            vec!["beep-node", "mesh", "leave"],
            vec!["beep-node", "update", "--check"],
            vec!["beep-node", "service", "install"],
            vec!["beep-node", "status"],
            vec!["beep-node", "st"],
            vec!["beep-node", "s"],
            vec!["beep-node", "version"],
            vec!["beep-node", "hook", "Stop"],
            vec!["beep-node", "mcp", "--session", "abc"],
        ] {
            assert!(
                command().try_get_matches_from(line.clone()).is_ok(),
                "`{}` should parse",
                line.join(" ")
            );
        }
    }
}
