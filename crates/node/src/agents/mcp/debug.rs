//! `beep_status`, `beep_logs` and `file_report`: an agent looks into Beep
//! itself and sends a problem report, from its own node. They read what a
//! problem report reads (`report/1`), with secrets redacted the same way:
//! every machine of the mesh.

use crate::agents::calls::command_name;
use super::reply::{done, fail, parse};
use super::server::ASK_LIMIT;
use crate::{
    agents::{Answered, Asking},
    link::{Offers, peers::Peers},
};
use beep_proto::SessionId;
use beep_proto::node::{
    agent::{AnswerOption, OptionKind},
    logs::Query as LogQuery,
    metrics::Query as MetricsQuery,
    tools::{Reply, Request, Tool},
};
use beep_report::{
    Answer, Period, SendError, Sender, Share, Target, collect, collect_within, draft, find_gh,
    gh_signed_in, issue, open_issue, redact, send, server_url, zip,
};
use chrono::{DateTime, Duration as ChronoDuration, SecondsFormat, Utc};
use regex::RegexBuilder;
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::{task::spawn_blocking, time::timeout};

const DEFAULT_MINUTES: u32 = 60;
const MAX_MINUTES: u32 = 7 * 24 * 60;
const DEFAULT_LINES: usize = 300;
const MAX_LINES: usize = 2000;
const SEND: &str = "send";
const SEND_AND_ISSUE: &str = "issue";

/// Answers one of the tools for the session that called it.
pub(super) async fn call(offers: &Offers, request: &Request) -> Reply {
    let Some(peers) = offers.reach.peers() else {
        return fail("This node is still starting; try again in a moment.");
    };
    let answered = match dispatch(offers, &peers, request).await {
        Ok(answered) => answered,
        Err(bad_args) => return fail(bad_args),
    };
    let (text, failed) = match answered {
        Ok(text) => (text, false),
        Err(text) => (text, true),
    };

    // Redaction runs regexes over whole logs: off the async workers.
    match spawn_blocking(move || redact(&text)).await {
        Ok(Ok(text)) => done(text, failed),
        Ok(Err(e)) => fail(format!("The answer was not sent: redacting it failed: {e}")),
        Err(e) => fail(format!("The answer was not sent: redacting it failed: {e}")),
    }
}

async fn dispatch(
    offers: &Offers,
    peers: &Peers,
    request: &Request,
) -> Result<Result<String, String>, String> {
    Ok(match request.tool {
        Tool::BeepStatus => Ok(status(peers).await),
        Tool::BeepLogs => logs(peers, parse(&request.args)?).await,
        Tool::BeepMetrics => metrics(peers, parse(&request.args)?).await,
        Tool::FileReport => file(offers, peers, &request.session, parse(&request.args)?).await,
        other => Err(format!("{other} is not a Beep debugging tool.")),
    })
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |now| i64::try_from(now.as_secs()).unwrap_or(i64::MAX))
}

/// `## name (kind, os version, Beep version): answer`.
fn heading(share: &Share) -> String {
    let os = [share.os.as_str(), share.os_version.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut about = vec![share.kind.to_string()];

    if !os.is_empty() {
        about.push(os);
    }

    if !share.version.is_empty() {
        about.push(format!("Beep {}", share.version));
    }

    format!(
        "## {} ({}): {}",
        share.name,
        about.join(", "),
        share.answer.label()
    )
}

/// Every machine's status, version and OS: no log.
async fn status(peers: &Peers) -> String {
    let shares = collect_within(peers.report_targets(), now(), 0).await;
    let mut text = String::new();

    for share in &shares {
        text.push_str(&heading(share));
        text.push('\n');
        text.push_str(share.status.trim_end());
        text.push_str("\n\n");
    }

    text
}

/// Whether `target` is the machine the agent named, by name or id.
fn named(target: &Target, name: &str) -> bool {
    command_name(&target.name) == command_name(name) || target.id.to_string() == name.trim()
}

fn find_target(peers: &Peers, machine: Option<&str>) -> Result<Target, String> {
    let me = peers.key().public();
    let name = machine.map(str::trim).unwrap_or_default();

    if let Some(target) = peers.report_targets().into_iter().find(|target| {
        if name.is_empty() {
            target.id == me
        } else {
            named(target, name)
        }
    }) {
        return Ok(target);
    }

    let names: Vec<String> = peers
        .devices()
        .iter()
        .map(|status| command_name(&status.device.name))
        .collect();

    Err(if names.is_empty() {
        format!("There is no machine {name}.")
    } else {
        format!(
            "There is no machine {name}. The machines: {}.",
            names.join(", ")
        )
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogsArgs {
    /// By its name in `beep_status`; this machine when left out.
    machine: Option<String>,
    minutes: Option<u32>,
    lines: Option<usize>,
    offset: Option<u32>,
    search: Option<String>,
    regex: Option<bool>,
    start: Option<String>,
    end: Option<String>,
}

struct QueryWindow {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
}

impl QueryWindow {
    fn new(
        start: Option<&str>,
        end: Option<&str>,
        lookback: ChronoDuration,
    ) -> Result<Self, String> {
        let parse = |value: &str| {
            let time = DateTime::parse_from_rfc3339(value).map_err(|_| {
                "Use UTC RFC 3339 timestamps, for example 2026-01-02T03:04:05Z.".to_owned()
            })?;

            if time.offset().local_minus_utc() != 0 {
                return Err("start and end must be in UTC (Z or +00:00).".to_owned());
            }

            Ok(time.with_timezone(&Utc))
        };
        let end = end.map(parse).transpose()?.unwrap_or_else(Utc::now);
        let start = match start.map(parse).transpose()? {
            Some(start) => start,
            None => end
                .checked_sub_signed(lookback)
                .ok_or("The metrics or logs lookback is outside the supported timestamp range.")?,
        };

        if start >= end || end.signed_duration_since(start) > ChronoDuration::days(7) {
            return Err("start must precede end by at most 7 days.".to_owned());
        }

        Ok(Self { start, end })
    }

    fn rfc3339(&self) -> (String, String) {
        (
            self.start.to_rfc3339_opts(SecondsFormat::Nanos, true),
            self.end.to_rfc3339_opts(SecondsFormat::Nanos, true),
        )
    }
}

fn validate_search(search: Option<&str>, regex: bool) -> Result<(), String> {
    if regex && search.is_none() {
        return Err("regex requires search.".to_owned());
    }

    if let Some(search) = search {
        if search.len() > 4096 {
            return Err("search must be at most 4096 bytes.".to_owned());
        }

        if regex {
            RegexBuilder::new(search)
                .size_limit(1024 * 1024)
                .build()
                .map_err(|e| format!("Invalid regex: {e}"))?;
        }
    }

    Ok(())
}

impl LogsArgs {
    fn minutes(&self) -> u32 {
        self.minutes
            .unwrap_or(DEFAULT_MINUTES)
            .clamp(1, MAX_MINUTES)
    }

    fn lines(&self) -> usize {
        self.lines.unwrap_or(DEFAULT_LINES).clamp(1, MAX_LINES)
    }

    fn query(&self) -> Result<Option<LogQuery>, String> {
        if self.offset.is_none()
            && self.search.is_none()
            && self.regex.is_none()
            && self.start.is_none()
            && self.end.is_none()
        {
            return Ok(None);
        }

        if self.start.is_some() && self.minutes.is_some() {
            return Err("Use start or minutes, not both.".to_owned());
        }

        let window = QueryWindow::new(
            self.start.as_deref(),
            self.end.as_deref(),
            ChronoDuration::minutes(i64::from(self.minutes())),
        )?;
        let offset = self.offset.unwrap_or(0);

        if offset > 100_000 {
            return Err("offset must be at most 100000.".to_owned());
        }

        validate_search(self.search.as_deref(), self.regex.unwrap_or(false))?;
        let (start, end) = window.rfc3339();

        Ok(Some(LogQuery {
            start,
            end,
            search: self.search.clone(),
            regex: self.regex.unwrap_or(false),
            lines: self.lines() as u32,
            offset,
            allow_untimed: self.start.is_none() && self.end.is_none(),
        }))
    }
}

/// One machine's log for the last `minutes`, its newest `lines` lines.
async fn logs(peers: &Peers, args: LogsArgs) -> Result<String, String> {
    let query = args.query()?;
    let minutes = args.minutes();
    let lines = args.lines();
    let target = find_target(peers, args.machine.as_deref())?;

    if let Some(query) = query {
        use beep_proto::node::logs::{QUERY_PROTOCOL, Reply};
        let client = target
            .client
            .map_err(|reason| format!("{}: {reason}", target.name))?;
        let reply = timeout(
            beep_report::ASK_LIMIT,
            client.call::<_, Reply>(QUERY_PROTOCOL, &query),
        )
        .await
        .map_err(|_| "Log query timed out.".to_owned())?
        .map_err(|e| {
            format!("Log query failed: {e:#}. This requires a machine supporting log-query/1.")
        })?;

        return match reply {
            Reply::Log { text } => Ok(format!(
                "{}'s Beep log, {} matching lines, offset {}, UTC [{}, {}):\n{text}",
                target.name,
                text.lines().count(),
                query.offset,
                query.start,
                query.end
            )),
            Reply::Failed { reason } => Err(reason),
            _ => Err("The machine returned an unknown log query reply.".to_owned()),
        };
    }

    let since = now() - i64::from(minutes) * 60;
    let Some(share) = collect(vec![target], since).await.pop() else {
        return Err("The machine gave nothing.".to_owned());
    };
    let note = match &share.answer {
        Answer::Answered => format!("the past {minutes} minutes"),
        Answer::OlderNode => "an older node: its latest lines, whatever their time".to_owned(),
        other => return Err(format!("{}: {}", share.name, other.label())),
    };
    let all: Vec<&str> = share.log.lines().collect();
    let kept = &all[all.len().saturating_sub(lines)..];

    if kept.is_empty() {
        return Ok(format!(
            "{}'s Beep log has nothing from {note}.",
            share.name
        ));
    }

    Ok(format!(
        "{}'s Beep log, {} lines of {note}:\n{}\n",
        share.name,
        kept.len(),
        kept.join("\n")
    ))
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricsArgs {
    machine: Option<String>,
    period: Option<String>,
    start: Option<String>,
    end: Option<String>,
    search: Option<String>,
    regex: Option<bool>,
    limit: Option<u32>,
    offset: Option<u32>,
}

impl MetricsArgs {
    fn query(&self, duration: Duration) -> Result<Option<MetricsQuery>, String> {
        if self.start.is_none()
            && self.end.is_none()
            && self.search.is_none()
            && self.regex.is_none()
            && self.limit.is_none()
            && self.offset.is_none()
        {
            return Ok(None);
        }

        if self.start.is_some() && self.period.is_some() {
            return Err("Use start or period, not both.".to_owned());
        }

        let window = QueryWindow::new(
            self.start.as_deref(),
            self.end.as_deref(),
            ChronoDuration::from_std(duration).map_err(|e| e.to_string())?,
        )?;

        if window.start.timestamp() < 0 {
            return Err("start must be on or after 1970.".to_owned());
        }

        validate_search(self.search.as_deref(), self.regex.unwrap_or(false))?;
        let limit = self.limit.unwrap_or(200);
        let offset = self.offset.unwrap_or(0);

        if !(1..=2048).contains(&limit) || offset > 2048 {
            return Err("limit must be between 1 and 2048, and offset at most 2048.".to_owned());
        }

        let (start, end) = window.rfc3339();

        Ok(Some(MetricsQuery {
            start,
            end,
            search: self.search.clone(),
            regex: self.regex.unwrap_or(false),
            limit,
            offset,
        }))
    }
}

async fn metrics(peers: &Peers, args: MetricsArgs) -> Result<String, String> {
    use beep_proto::node::metrics::{PROTOCOL, QUERY_PROTOCOL, Reply, Request};
    let period = args.period.as_deref().unwrap_or("1h");
    let duration = humantime::parse_duration(period).map_err(|e| {
        format!("Invalid metrics period {period:?}. Use a duration such as 15m, 1h or 1d: {e}")
    })?;

    if duration < Duration::from_secs(1)
        || duration > Duration::from_secs(beep_proto::node::metrics::MAX_PERIOD)
    {
        return Err("The metrics period must be between 1s and 7d.".to_owned());
    }

    let query = args.query(duration)?;
    let target = find_target(peers, args.machine.as_deref())?;
    let client = target
        .client
        .map_err(|reason| format!("{}: {reason}", target.name))?;
    let until = u64::try_from(now()).map_err(|e| e.to_string())?;
    let request = Request {
        since: until.saturating_sub(duration.as_secs()),
        until,
    };
    let protocol = if query.is_some() {
        QUERY_PROTOCOL
    } else {
        PROTOCOL
    };
    let reply = timeout(Duration::from_secs(30), async {
        match &query {
            Some(query) => client.call::<_, Reply>(protocol, query).await,
            None => client.call::<_, Reply>(protocol, &request).await,
        }
    })
    .await
    .map_err(|_| "The machine did not answer within 30 seconds.".to_owned())?
    .map_err(|e| {
        format!(
            "{}: {e:#}. Metrics collection requires a version supporting {protocol}.",
            target.name
        )
    })?;

    let window = query.as_ref().map_or_else(
        || period.to_owned(),
        |query| format!("UTC [{}, {})", query.start, query.end),
    );

    match reply {
        Reply::Summary { text } => Ok(format!(
            "{}'s Beep metrics, requested {window}:\n{text}",
            target.name
        )),
        Reply::Failed { reason } => Err(format!("{}: {reason}", target.name)),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReportArgs {
    /// The agent's findings: what went wrong and why.
    description: String,
    /// `hour`, `today` or `day`.
    period: Option<String>,
}

/// Puts the report to the user, then sends it as the desktop's Report a
/// problem does, and gives its reference.
async fn file(
    offers: &Offers,
    peers: &Peers,
    session: &SessionId,
    args: ReportArgs,
) -> Result<String, String> {
    let description = args.description.trim().to_owned();

    if description.is_empty() {
        return Err("The description is empty: say what you found.".to_owned());
    }

    let period = match args.period.as_deref().map(str::trim) {
        None | Some("") => Period::Today,
        Some(period) => period
            .parse::<Period>()
            .map_err(|_| format!("The period is hour, today or day, not {period}."))?,
    };
    let desktop = peers.is_desktop();
    let covers = match period {
        Period::Hour => "the past hour",
        Period::Today => "today",
        Period::Day => "the past day",
    };
    let option = |id: &str, name: &str, kind| AnswerOption {
        id: id.to_owned(),
        name: name.to_owned(),
        kind,
    };
    let gh = match find_gh() {
        Some(gh) if gh_signed_in(&gh).await => Some(gh),
        _ => None,
    };
    let mut detail = format!(
        "{description}\n\nWith Beep's logs from {covers}, of every machine, secrets removed."
    );
    let mut options = vec![option(SEND, "Send", OptionKind::AllowOnce)];

    if gh.is_some() {
        detail.push_str(
            " Send and open issue also opens a public GitHub issue with your gh: this \
             description, the reference and versions, no logs.",
        );
        options.insert(
            0,
            option(SEND_AND_ISSUE, "Send and open issue", OptionKind::AllowOnce),
        );
    }

    options.push(option("keep", "Don't send", OptionKind::RejectOnce));
    let question = Asking {
        text: "Send a problem report to Beep?".to_owned(),
        detail: Some(detail),
        options,
    };
    let with_issue = match timeout(ASK_LIMIT, offers.agents.ask(session, question)).await {
        Ok(Ok(Answered::Picked(option))) if option.id == SEND => false,
        Ok(Ok(Answered::Picked(option))) if option.id == SEND_AND_ISSUE => true,
        Ok(Ok(_)) => {
            return Err(
                "The user did not send the report. Do not try again unless they ask.".to_owned(),
            );
        }
        Ok(Err(e)) => return Err(format!("Not sent: {e:#}")),
        Err(_) => return Err("Not sent: the user did not answer within an hour.".to_owned()),
    };

    let since = period.since_now();
    let shares = collect(peers.report_targets(), since).await;
    let key = peers.key();
    let sender = Sender {
        id: key.public(),
        mesh: peers.status().await.ok().and_then(|status| status.mesh),
        app: if desktop { "desktop" } else { "node" },
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
    };
    // Redacting every share and deflating them is CPU work: off the async workers.
    let built = spawn_blocking(move || {
        let draft = draft(&description, period, since, sender, shares)?;
        let answers: Vec<String> = draft
            .manifest
            .machines
            .iter()
            .map(|machine| format!("{} {}", machine.name, machine.answer.label()))
            .collect();
        zip(&draft).map(|body| (body, answers, draft.manifest))
    })
    .await
    .map_err(|e| format!("Not sent: {e}"))?;
    let (body, answers, manifest) = built.map_err(|e| format!("Not sent: {e}"))?;
    let url = server_url().map_err(|e| format!("Not sent: {e}"))?;
    let receipt = send(&url, body, &key).await.map_err(|e| match e {
        SendError::Refused(words) => format!("Not sent: {words}"),
        e => format!("Not sent: {e}"),
    })?;
    let mut text = format!(
        "Sent. Reference: {}\nMachines: {}.",
        receipt.reference,
        answers.join(", ")
    );

    if let Some(gh) = gh
        && with_issue
    {
        // The report reached Beep: a failed issue is said, not a failed call.
        let opened = match issue(&manifest, &receipt.reference) {
            Ok(issue) => open_issue(&gh, &issue).await.map_err(|e| e.to_string()),
            Err(e) => Err(e.to_string()),
        };

        match opened {
            Ok(url) => text.push_str(&format!("\nIssue: {url}")),
            Err(e) => text.push_str(&format!("\nThe GitHub issue did not open: {e}")),
        }
    }

    Ok(text)
}

#[cfg(test)]
#[path = "../../tests/agents_mcp_debug.rs"]
mod tests;
