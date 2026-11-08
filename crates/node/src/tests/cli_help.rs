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
