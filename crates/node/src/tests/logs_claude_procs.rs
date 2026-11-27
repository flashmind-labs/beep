use super::*;

const ID: &str = "6dc7cadf-1ac6-487c-8730-2fc6d392f852";
const OLD: &str = "c9cd0baf-082d-48c9-844f-9ec2cac3b754";

#[test]
fn lsof_output_pairs_pids_with_folders() {
    let text = "p101\nfcwd\nn/srv/project\np103\nfcwd\nn/srv/other dir\n";
    assert_eq!(
        lsof_folders(text),
        [
            (101, PathBuf::from("/srv/project")),
            (103, PathBuf::from("/srv/other dir"))
        ]
    );
}

/// What `ps` shows on a machine where the daemon parked an interactive
/// session into a background one, run by the versioned binary.
#[test]
fn the_versioned_binary_counts_and_the_daemons_helpers_do_not() {
    let listing = format!(
        " 8039 claude --dangerously-skip-permissions\n\
         8253 claude bg-pty-host --bg-pty-host /tmp/cc-daemon/pty/6dc7cadf.sock 245 69 -- /home/u/.local/share/claude/versions/2.1.311 --session-id {ID} --fork-session --resume /home/u/.claude/projects/-home-u-proj/{OLD}.jsonl\n\
         8271 /home/u/.local/share/claude/versions/2.1.311 --session-id {ID} --fork-session --resume /home/u/.claude/projects/-home-u-proj/{OLD}.jsonl --model claude-fable-5-1\n\
         9626 /home/u/.local/bin/claude daemon run --origin transient\n\
         9646 claude bg-pty-host --bg-pty-host /tmp/cc-daemon/spare/f9.pty.sock 200 50 -- /home/u/.local/share/claude/versions/2.1.312 --bg-spare /tmp/cc-daemon/spare/f9.claim.sock\n\
         9655 claude bg-spare --bg-spare /tmp/cc-daemon/spare/f9.claim.sock\n\
         9700 /opt/tools/versions/1.2.3 --session-id {ID}\n\
         9701 node /home/u/claude-code/cli.js\n\
         9702 beep-node run\n"
    );
    assert_eq!(
        claude_sessions(listed(&listing), |_| false),
        [(8039, None), (8271, Some(SessionId::from(ID)))]
    );
}

/// A program path with spaces reads as Claude Code only when it is a
/// file, so another program given a path ending in `claude` does not.
#[test]
fn a_program_path_with_spaces_is_claude_when_it_is_a_file() {
    let listing = format!(
        "41 /Users/Jane Doe/.local/bin/claude --resume {OLD}\n\
         42 /Users/Jane Doe/.local/share/claude/versions/2.1.319-beta -r /Users/Jane Doe/.claude/projects/x/{OLD}.jsonl --model opus\n\
         43 /Users/Jane Doe/.local/bin/claude.sh --resume {OLD}\n\
         44 git -C /Users/Jane Doe/src/claude status\n\
         45 nvim /Users/jane/claude\n\
         46 rg foo /Users/jane/.local/share/claude/versions/2.1.311\n"
    );
    let is_file = |program: &str| program.starts_with("/Users/Jane Doe/.local/");
    assert_eq!(
        claude_sessions(listed(&listing), is_file),
        [
            (41, Some(SessionId::from(OLD))),
            (42, Some(SessionId::from(OLD)))
        ]
    );
    assert_eq!(claude_sessions(listed(&listing), |_| false), []);
}

#[test]
fn the_session_named_comes_from_the_arguments() {
    assert_eq!(session_named(""), None);
    assert_eq!(session_named("--resume"), None);
    assert_eq!(session_named("--resume --model opus"), None);
    assert_eq!(session_named(&format!("--resume {OLD}")), Some(OLD.into()));
    assert_eq!(
        session_named(&format!("--resume=/x/{OLD}.jsonl")),
        Some(OLD.into())
    );
    // A fork of a session is a new one, named only by `--session-id`.
    assert_eq!(
        session_named(&format!("--fork-session --resume {OLD}")),
        None
    );
    assert_eq!(
        session_named(&format!("--session-id {ID} --fork-session --resume {OLD}")),
        Some(ID.into())
    );
    assert_eq!(session_named("--session-id nope"), None);
    assert_eq!(
        session_named(&format!(
            "--resume /Users/Jane Doe/.claude/x/{OLD}.jsonl --model opus"
        )),
        Some(OLD.into())
    );
    assert_eq!(
        session_named(&format!("--session-id={ID} -p use --resume {OLD}")),
        Some(ID.into())
    );
}

/// The pids and command lines in `ps -o pid=,args=` style `listing`.
fn listed(listing: &str) -> impl Iterator<Item = (u32, &str)> {
    listing.lines().filter_map(|line| {
        let (pid, args) = line.trim().split_once(char::is_whitespace)?;
        Some((pid.parse().ok()?, args))
    })
}
