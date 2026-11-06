//! `beep-node report`: every machine's log, status, version and OS, through
//! the running node, shown and then sent to Beep.

use anyhow::{Context, Result, bail};
use beep_mesh::DeviceKind;
use beep_node::{
    link::{
        peers::{self, ReportError},
        state::State,
    },
    machine::{env::os_version, service::logs::Logs},
};
use beep_proto::node::report::MAX_BYTES;
use beep_report::{
    Answer, Draft, Issue, Period, SendError, Sender, Share, draft, find_gh, gh_signed_in, issue,
    open_issue, send, server_url, zip,
};
use chrono::Local;
use iroh::EndpointId;
use std::{env::consts::OS, io::Write, path::Path};
use tokio::{
    io::{AsyncBufReadExt, BufReader, Lines, Stdin, stdin},
    task::spawn_blocking,
};

use crate::cli::status::VERSION;

/// What `report` says of this machine when no node runs from the folder.
const NO_NODE: &str = "no node runs here";

pub(crate) async fn run(dir: &Path, period: Period, yes: bool, text: Option<String>) -> Result<()> {
    let state =
        State::load(dir).context("this machine has no Beep key to sign with; nothing was sent")?;
    let mut input = BufReader::new(stdin()).lines();
    let text = match text {
        Some(text) => text,
        None => ask(&mut input, "What happened?").await?,
    };
    let since = period.since(Local::now());
    let id = state.key.public();
    let (shares, mesh) = match peers::collect_report(dir, since).await {
        Ok(Some(shares)) => {
            // The mesh's id only labels the report; one the node does not give is left out.
            let mesh = peers::local_status(dir)
                .await
                .ok()
                .flatten()
                .and_then(|status| status.mesh);
            (shares, mesh)
        }
        Ok(None) => {
            let share = own_share(id, &state.name, since, Answer::Answered, NO_NODE).await;
            (vec![share], None)
        }
        Err(e) => {
            let why = format!("{e:#}");
            let share = own_share(id, &state.name, since, fallback_answer(&e), &why).await;
            (vec![share], None)
        }
    };
    let sender = Sender {
        id,
        mesh,
        app: "node",
        app_version: VERSION.to_owned(),
    };
    // Redaction runs regexes over every share's log: off the async workers.
    let draft = spawn_blocking(move || draft(&text, period, since, sender, shares)).await??;
    print!("{}", listing(&draft));
    if !yes && !confirm(&mut input, "Send this to Beep? [y/N]").await? {
        println!("Nothing was sent.");
        return Ok(());
    }
    let (draft, body) = spawn_blocking(move || {
        let body = zip(&draft);
        (draft, body)
    })
    .await?;
    let body = body?;
    let receipt = match send(&server_url()?, body, &state.key).await {
        Ok(receipt) => receipt,
        Err(SendError::Refused(words)) => bail!("{words}"),
        Err(e) => return Err(e.into()),
    };
    println!("Sent. Reference: {}", receipt.reference);
    // `--yes` answers no here: an issue is public and opened only when asked.
    if yes {
        return Ok(());
    }
    let Some(gh) = find_gh() else {
        return Ok(());
    };
    if !gh_signed_in(&gh).await {
        return Ok(());
    }
    // The report reached Beep: a failed issue is said, not a failed command.
    let issue = match issue(&draft.manifest, &receipt.reference) {
        Ok(issue) => issue,
        Err(e) => {
            eprintln!("the report was sent, but the GitHub issue could not be written: {e}");
            return Ok(());
        }
    };
    print!("{}", issue_text(&issue));
    if !confirm(&mut input, "Report it on GitHub too? [y/N]").await? {
        return Ok(());
    }
    match open_issue(&gh, &issue).await {
        Ok(url) => println!("{url}"),
        Err(e) => eprintln!("the report was sent, but the GitHub issue did not open: {e}"),
    }
    Ok(())
}

/// The public issue as it would open, shown before asking.
fn issue_text(issue: &Issue) -> String {
    format!("\n{}\n\n{}\n", issue.title, issue.body.trim_end())
}

/// How this machine's share answers when the running node gave none.
fn fallback_answer(e: &anyhow::Error) -> Answer {
    match e.downcast_ref::<ReportError>() {
        Some(ReportError::Silent(_)) => Answer::Silent,
        Some(ReportError::Older) => Answer::OlderNode,
        None => Answer::Failed {
            reason: format!("{e:#}"),
        },
    }
}

/// This machine's share read here, for when the running node gives none;
/// `status` says why.
async fn own_share(id: EndpointId, name: &str, since: i64, answer: Answer, status: &str) -> Share {
    // A log that cannot be read still leaves the rest worth sending.
    let max = usize::try_from(MAX_BYTES).unwrap_or(usize::MAX);
    let log = spawn_blocking(move || Logs::service().window(since, max))
        .await
        .map_err(anyhow::Error::from)
        .and_then(|read| read)
        .unwrap_or_else(|e| format!("the log could not be read: {e:#}"));
    Share {
        id,
        name: name.to_owned(),
        kind: DeviceKind::Node,
        answer,
        log,
        status: status.to_owned(),
        version: VERSION.to_owned(),
        os: OS.to_owned(),
        os_version: os_version().await,
    }
}

/// Each file with its size, then how each machine answered.
fn listing(draft: &Draft) -> String {
    let mut out = String::from("Files:\n");
    for file in &draft.manifest.files {
        out.push_str(&format!("  {}  {}\n", file.path, size(file.bytes)));
    }
    out.push_str("Machines:\n");
    for machine in &draft.manifest.machines {
        out.push_str(&format!("  {}  {}\n", machine.name, machine.answer.label()));
    }
    out
}

fn size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{} KB", bytes.div_ceil(1024)),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

/// Prints `question` and reads one line; nothing typed reads as "".
async fn ask(input: &mut Lines<BufReader<Stdin>>, question: &str) -> Result<String> {
    print!("{question} ");
    std::io::stdout().flush()?;
    Ok(input
        .next_line()
        .await?
        .unwrap_or_default()
        .trim()
        .to_owned())
}

async fn confirm(input: &mut Lines<BufReader<Stdin>>, question: &str) -> Result<bool> {
    let answer = ask(input, question).await?;
    Ok(matches!(answer.to_lowercase().as_str(), "y" | "yes"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::SecretKey;

    #[tokio::test]
    async fn a_silent_node_gives_this_machines_share_marked_silent() {
        let silent = anyhow::Error::from(ReportError::Silent(30));
        let now = Local::now().timestamp();
        let share = own_share(
            SecretKey::generate().public(),
            "build-box",
            now,
            fallback_answer(&silent),
            &format!("{silent:#}"),
        )
        .await;
        assert_eq!(
            share.status,
            "the running node did not answer in 30 seconds"
        );
        assert_eq!(share.answer, Answer::Silent);
        assert_eq!(share.answer.label(), "did not answer");
        assert_eq!(share.version, VERSION);
    }

    #[test]
    fn other_collect_failures_read_as_failed_with_why() {
        let older = anyhow::Error::from(ReportError::Older);
        assert_eq!(fallback_answer(&older), Answer::OlderNode);
        let broken = anyhow::anyhow!("reading the node's report");
        assert_eq!(
            fallback_answer(&broken),
            Answer::Failed {
                reason: "reading the node's report".to_owned()
            }
        );
    }

    #[test]
    fn the_issue_shows_its_title_and_body() {
        let issue = Issue {
            title: "Problem report R-1".to_owned(),
            body: "> it broke\n\nReference: R-1\n".to_owned(),
        };
        assert_eq!(
            issue_text(&issue),
            "\nProblem report R-1\n\n> it broke\n\nReference: R-1\n"
        );
    }
}
