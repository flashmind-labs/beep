//! Keeps the server readable by the apps already out there.
//!
//! `tests/wire/<wire>/` holds, for every request, the msgpack of sample
//! values of the request and of its reply, as clients reading that wire
//! revision send and read them. The test checks that each request fixture
//! still decodes, so a released app's requests keep working, and that the
//! reply samples still encode to the same bytes, so a released app can still
//! read what the server sends. Fixtures are append-only: run with
//! `BEEP_WIRE_BLESS=1` to write the ones a new request or a new `WIRE` lacks.
//! It never overwrites a fixture.

use crate::{each_request, link::LinkOffer, *};
use iroh::SecretKey;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

/// Values that cover a type's wire shapes: every variant of an enum, and
/// every sample of each field somewhere.
pub(crate) trait Sample: Sized {
    /// What a client reading `wire` may be sent; never empty.
    fn samples(wire: u32) -> Vec<Self>;
}

pub(crate) trait Visit {
    fn request<R: Rpc + Sample, T: Serialize + Sample>(&mut self);
}

/// The `k`th sample of `T`, wrapping around; sets `more` while `T` has
/// samples past `k`.
pub(crate) fn pick<T: Sample>(wire: u32, k: usize, more: &mut bool) -> T {
    let mut samples = T::samples(wire);
    assert!(!samples.is_empty(), "a type with no samples");
    if k + 1 < samples.len() {
        *more = true;
    }
    let i = k % samples.len();
    samples.swap_remove(i)
}

/// Pushes onto `out` values of the struct or variant `ctor` until each field
/// has taken every one of its samples.
macro_rules! sample_each {
    ($out:ident, $wire:ident, [$($ctor:tt)+] { $($field:ident),* }) => {{
        let _ = $wire;
        for k in 0usize.. {
            let _ = k;
            #[allow(unused_mut)]
            let mut more = false;
            $out.push($($ctor)+ { $($field: $crate::compat::pick($wire, k, &mut more)),* });
            if !more {
                break;
            }
        }
    }};
}

/// `Sample` for a plain struct, listing all its fields.
macro_rules! sample_struct {
    ($($name:ident { $($field:ident),* $(,)? })*) => {$(
        impl $crate::compat::Sample for $name {
            fn samples(wire: u32) -> Vec<Self> {
                let mut out = Vec::new();
                sample_each!(out, wire, [$name] { $($field),* });
                out
            }
        }
    )*};
}

sample_struct! {
    ImageRef { id, mime, width, height }
    SentImages { message, images }
    TurnAgent { message, agent }
    FilePart { data, size }
    Upload { name, mime, data }
    VoiceNote { text, audio }
    FileRef { id, name, mime, size }
    SentFiles { message, files }
    Approval { app, action, fields }
    ApprovalField { label, value }
    PendingApproval { run_id, query_id, approval }
    ChatApproval { session_id, session_title, pending }
    ProjectAgent { project_id, name, chat_id, open_tasks, open_workers }
    ProjectTask { id, slug, title, body, state, depends_on, issue_number, issue_url, branch, pr_url, created_at, updated_at }
    ProjectIssue { number, title, labels, author, opened_at, comments, url }
    ProjectOverview { project_id, name, tasks, workers, latest }
    TaskCounts { planned, running, blocked, review, done }
    InstructionsVersion { version, text, note, saved_at }
    MachineAgentFiles { machine_id, machine_name, files, error }
    AgentFile { agent, path, text, synced }
    Terminal { id, project, machine_id, machine_name, name, command, cwd, state, started_at }
    TerminalOutput { terminal, lines, earlier }
    ProjectBranches { machine_id, machine_name, default_branch, branches }
    BranchPlace { branch, machine_id }
    ImportPreview { project, checkouts, default_branch, branches, note }
    ImportCheckout { machine_id, machine_name, folder, remote }
    ImportFork { project, fork, branches }
    WorkBranch { name, pr, pr_title, ahead, worktree, in_checkout, committed_at }
    ProjectBranch { name, worktree, ahead, behind, head, subject, committed_at, local, pushed }
    BranchReview { id, project, branch, base, head, head_at, started_at, machine_name, changed_lines, files, merge, pr, shards, findings }
    ReviewPr { number, url, checks }
    ReviewHead { head, stale }
    ReviewShard { area, files, lines, agent, state, found }
    ReviewFinding { id, area, severity, title, file, line, why, verdict, dismissed }
    AgentAccounts { claude, codex_key, machines }
    AgentToken { last4, created_at, unreadable }
    AgentMachine { machine_id, machine_name, online, claude, codex, claude_sessions, codex_signed_in }
    CodexLogin { task, url, code, expires_at }
    IssueStart { agent, node_id }
    ProjectThread { kind, repo, number, title, url, last_other, needs_reply, updated_at }
    ProjectPullRequest { number, title, url, author, draft, mine, created_at, updated_at }
    PullRequestHead { number, branch, fork }
    ThreadChat { chat_id, kind, repo, number, title }
    ThreadView { kind, number, title, url, author, body, needs_reply, comments, earlier }
    ThreadPost { author, at, body }
    ThreadDetail { kind, number, title, url, author, state, draft, labels, opened_at, body, comments, earlier, review, checks }
    AgentImage { name, file_id, mime, key }
    ProjectGroup { id, name, paths }
    AgentImageData { key, mime, data }
    AgentFileChunk { size, mime, data }
    WorkFolder { path, entries, more, git }
    WorkTree { folders, truncated }
    WorkDiff { root, base, merge_base, working_tree, files, additions, deletions, more }
    DiffFile { path, old_path, status, additions, deletions, hunks, left_out }
    DiffHunk { old_start, old_lines, new_start, new_lines, section, lines }
    DiffLine { kind, text, no_eol }
    CustomAgent { name, harness, scope, description, model, tools, color, path, prompt, prompt_cut, error }
    WorkEntry { name, folder, size, ignored, change }
    ProjectWorker { id, task_id, machine_id, machine_name, agent, session, folder, state, last_output, persona, started_at, ended_at }
    AppUpdate { id, summary, app, app_name, at }
    Credential { id, kind, last4, account, env_var, to_machines, created_at, unreadable }
    GitHubAccount { id, login, orgs, default, last4, created_at, unreadable }
    Models { choices, selected, thinking, auto, auto_available }
    AutoPick { model, thinking }
    Agents { enabled, agents }
    AgentInfo { id, name, when, model, thinking }
    ModelChoice { id, name, input_price, output_price, sees_images, default }
    AppPermissions { level, actions }
    AppAction { tool, label, allowed }
    HistoryTool { name, call_id, output, success }
    ToolLabel { call_id, description }
    Finding { id, kind, summary, steps, cause, cause_detail, fix, status, seen, first_seen, last_seen, session_id, first_message, last_message }
    MachineDetails { id, apps, gpus, capabilities }
    MachineApp { name, version }
    MachineGpu { vendor, model, vram_mb, driver }
    MachineStats { id, sampled_at, disks, memory_used_mb, memory_total_mb, load }
    MachineDisk { path, free_bytes, total_bytes }
    MachineTraffic { id, updated_at, counters }
    TrafficCounter { key, bytes }
    MachineSession { machine_id, machine_name, id, agent, title, cwd, status, doing, started_at, last_activity, external, question }
    AgentQuestion { id, text, detail, options }
    AgentOption { id, name, allow }
    AgentHost { machine_id, machine_name, agents, workspace, home, capabilities }
    StartedAgent { session, why }
    MachinePick { machine_id, machine_name, why }
    AgentModels { pick, current, models }
    AgentModel { id, name, description }
    AgentSessionView { session, plan, files, last_message, error, queued, turns, entries, earlier }
    AgentSessionUsage { context, tokens, cost_usd }
    AgentContext { used, size }
    AgentPlanStep { content, status }
    AgentEntry { kind, text }
    AppIntegration { slug, name, description, logo_url, connected, configured, kind }
    ConnectLink { url }
    AppAccount { id, address }
    McpServer { id, name, url, connected, sign_in, tools }
    McpConnect { server, auth_url }
    McpTools { tools, limit }
    McpTool { name, description, enabled, usable }
    PromptSuggestion { title, prompt }
    PromptSuggestions { suggestions, personal }
    Usage { prompt_tokens, completion_tokens, total_tokens, cache_read_tokens, cache_creation_tokens }
    Event { seq, event }
    Run { id, session_id, status }
    Session { id, title, pinned, updated_at }
    SharedLink { token, url, title, shared_at }
    SharedSession { title, shared_at, entries }
    SessionMatch { session, snippet, from_user }
    Memory { id, content, category, source, updated, valid_from, valid_until, quote }
    MemoryRead { id, source, finished_at, memories }
    GroupReminder { id, label, message, schedule, next_run_at, set_by }
    Skill { id, name, description, body, group_id, group_name, enabled, uses, last_used, updated_at, edited, can_undo }
    Place { id, name, latitude, longitude, radius, address, created_at, here, last_here_at, routines }
    Cron { id, label, prompt, schedule, enabled, created_at, run_once, last_run_at, next_run_at }
    LinkedDevice { id, name, kind, primary, this_device, linked_at }
    Machine { id, name, online, seen_at, version, env }
    MachineEnv { os, os_version, arch, hostname, cpus, memory_mb, home, tools, logins, avds }
    MachineTool { name, version }
    MachineLogin { tool, signed_in, account }
    ScopedMemory { id, scope }
    WorkItem { id, kind, task, started_at, step, outcome }
    WorkOutcome { ok, text }
    Grant { id, routine_id, routine, app, action, fields, per_run, expires_at, created_at, last_used_at }
    DeviceInvite { code, uri, expires_at }
    JoinPreview { name, kind }
    Group { id, name, members, last, updated_at, beep_typing, pinned, apps_shared }
    GroupMember { name, you, creator, joined_at }
    GroupChat { group, messages }
    GroupMessage { seq, author, content, timestamp }
    GroupLink { token, url }
    GroupPreview { id, name, members, joined }
    Notice { title, body, session_id, app, approval, changed, group }
    ChatInfo { id, kind, title, pinned, members, apps_shared }
    ChatMessage { id, turn_id, timestamp, body }
    TurnInfo { model, auto, reasoning, memory_score }
    UpdatePage { updates, last, more, reset }
    Chart { title, kind, x, categories, unit, series, stock, source }
    ChartSeries { name, points }
    ChartPoint { x, y }
    StockInfo { symbol, name, exchange, currency, range, last, previous }
    RouteCard { routes }
    RouteOption { departs, arrives, minutes, transfers, live, legs }
    RouteLeg { mode, line, name, color, text_color, headsign, from, to, from_track, to_track, departs, arrives, minutes, stops, meters, points, from_code, to_code, between }
    RouteStop { name, code, time, at }
    RouteCardV1 { routes }
    RouteOptionV1 { departs, arrives, minutes, transfers, live, legs }
    RouteLegV1 { mode, line, name, color, text_color, headsign, from, to, from_track, to_track, departs, arrives, minutes, stops, meters, points }
    RoutePoint { latitude, longitude }
    Research { id, topic, status, agents, entries, sources, draft, started_at, finished_at, round }
    ResearchAgent { id, name, role, model, round }
    ResearchSummary { session_id, topic, status, round, sources, started_at, finished_at }
    ResearchEntry { seq, agent, body, timestamp }
    ResearchSource { number, url, title, quote, agent }
    WebSource { url, title }
    TurnSources { message, sources }
    ProjectSync { project, pinned, machines }
    MachineSync { machine_id, machine_name, folder, status, files, bytes, pending, conflicts, rescan_secs, hint, error }
    SyncSkipped { paths, through_git, truncated }
    SkippedFile { path, folder, rule }
    NodeToken { token, expires_at }
    ImportedProject { project, url, machine_id, machine_name, folder, cloned, imported_at }
    MachineFolder { path, parent, exists, folders, more, empty, origin }
    FoundProject { project, remote, places, agents, last_activity }
    FoundPlace { machine_id, machine_name, folder }
    FoundAgent { agent, sessions }
    HostRepos { host, kind, repos, error }
    HostRepo { name, url, updated_at }
    ProjectMachine { machine_id, machine_name, online, folder, checking, checked_at }
    SyncthingPlace { machine_id, folder, label }
    AgentHarnesses { machine_id, machine_name, checked_at, harnesses }
    AgentHarness { agent, installed, runnable, version, latest, program, home_var, home_default, sign_in, install, settings }
    HarnessSettings { enabled, program, home, args, env }
    HarnessEnv { name, value }
    MachineHarnessSettings { machine_id, machine_name, settings }
    FlshSettings { zdr, quantizations, sort, allow_fallbacks, autocompact, models }
    FlshAutoCompact { enabled, tokens }
    FlshModelRule { model, autocompact }
    ProjectSyncDetail { project, pinned, paused, machines }
    MachineSyncDetail { sync, paused, progress, waiting, can_resolve }
    SyncProgress { files_done, files_total, bytes_done, bytes_total, rate, eta_secs }
    GitItems { items, hosts }
    GitHost { host, kind, account, error }
    UsageView { limits, today, harnesses, week }
    UsageReport { limits, today, week, month }
    PeriodSpend { total, tokens, projects, harnesses }
    HarnessSpend { agent, cost_usd, tokens, models }
    ModelSpend { model, tokens }
    PlanLimits { agent, plan, machine_name, as_of, windows }
    PlanWindow { minutes, used_percent, resets_at, scope }
    ProjectSpend { project, cost_usd, unpriced }
    AgentTokens { agent, tokens }
    SpendReport { limits, today, week, month }
    SpendPeriod { cost_usd, metered_tokens, subscription_tokens, projects, harnesses }
    ProjectBill { project, cost_usd, metered_tokens, subscription }
    HarnessBill { agent, subscription, cost_usd, tokens, models }
    GitItem { host, repo, number, kind, title, url, updated_at, authored, assigned, review_requested, draft, review, checks_failing, comments }
    ProjectLists { pull_requests, prs_error, issues, issues_error, synced_at }
    LabeledProjectLists { lists, pr_labels }
    LabeledGitItems { found, labels }
    ProjectRuns { runs, checks, synced_at, error, rate_limited_until }
    ProjectChange { live, polling_why, lists, runs }
    WorkflowRun { id, number, workflow, title, branch, event, actor, state, url, started_at, finished_at, progress, pull_requests }
    RunProgress { job, job_started_at, jobs_done, jobs, step }
    StepProgress { number, of, name, started_at }
    PrChecks { number, state, run_id, progress }
    ProjectRunDetail { run, jobs, log, error, rate_limited_until }
    RunJob { id, name, state, started_at, finished_at, steps, url }
    RunStep { number, name, state, started_at, finished_at }
    RunLog { job, step, text, truncated }
    HostedRuns { runs, checks, hosts, error }
    HostedRun { host, run }
    HostedChecks { host, checks }
    RunsHost { host, kind, repo, primary, synced_at, error, rate_limited_until }
    MachineUpdate { from, to, restarting, note }
    UpdatableMachine { id, agent_sessions }
    ToolUpdate { machine_id, tool, version, latest }
    SshAdd { machine_id, system, error, output }
}

macro_rules! sample_values {
    ($($ty:ty => [$($value:expr),+ $(,)?])*) => {$(
        impl Sample for $ty {
            fn samples(_: u32) -> Vec<Self> {
                vec![$($value),+]
            }
        }
    )*};
}

sample_values! {
    () => [()]
    bool => [true, false]
    u8 => [7]
    u32 => [42]
    u64 => [70_000]
    i32 => [-2]
    i64 => [1_760_000_000_000]
    f64 => [1.5]
    String => ["text".to_owned()]
    AgentSlug => [AgentSlug::from("text")]
    ModelId => [ModelId::from("text")]
    FlshModel => ["openrouter:vendor/model".parse().expect("a valid model")]
    FlshSort => [FlshSort::Price, FlshSort::Throughput, FlshSort::Latency]
    FlshQuantization => [
        FlshQuantization::Int4,
        FlshQuantization::Int8,
        FlshQuantization::Fp4,
        FlshQuantization::Fp6,
        FlshQuantization::Fp8,
        FlshQuantization::Fp16,
        FlshQuantization::Bf16,
        FlshQuantization::Fp32,
        FlshQuantization::Unknown,
    ]
    Uuid => [Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef)]
    SystemTime => [UNIX_EPOCH + Duration::from_millis(1_760_000_000_123)]
    ReasoningLevel => [
        ReasoningLevel::Off,
        ReasoningLevel::Low,
        ReasoningLevel::Medium,
        ReasoningLevel::High,
    ]
    ChartKind => [ChartKind::Line, ChartKind::Area, ChartKind::Bar]
    ChartAxis => [ChartAxis::Time, ChartAxis::Category]
    ChartRange => [
        ChartRange::Day,
        ChartRange::Week,
        ChartRange::Month,
        ChartRange::HalfYear,
        ChartRange::YearToDate,
        ChartRange::Year,
        ChartRange::FiveYears,
        ChartRange::Max,
    ]
}

impl<T: Sample> Sample for Option<T> {
    fn samples(wire: u32) -> Vec<Self> {
        let mut out: Vec<Self> = T::samples(wire).into_iter().map(Some).collect();
        out.push(None);
        out
    }
}

impl<T: Sample> Sample for Vec<T> {
    fn samples(wire: u32) -> Vec<Self> {
        vec![T::samples(wire)]
    }
}

impl Sample for SecretText {
    fn samples(_: u32) -> Vec<Self> {
        vec![SecretText::new("secret-1234")]
    }
}

impl Sample for LinkOffer {
    fn samples(_: u32) -> Vec<Self> {
        let device = SecretKey::from_bytes(&[3; 32]);
        let server = SecretKey::from_bytes(&[4; 32]).public();
        let at = UNIX_EPOCH + Duration::from_secs(1_760_000_000);
        vec![LinkOffer::sign(&device, server, "Laptop", at).expect("the name is valid")]
    }
}

const BLESS: &str = "BEEP_WIRE_BLESS";
const ERRORS: &str = "error.reply";

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/wire")
}

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    rmp_serde::to_vec(value).expect("samples encode")
}

/// One wire revision's fixtures and what checking them found.
struct Revision {
    wire: u32,
    dir: PathBuf,
    files: BTreeMap<String, Vec<u8>>,
    used: BTreeSet<String>,
    bless: bool,
    written: Vec<String>,
    problems: Vec<String>,
}

impl Revision {
    fn load(wire: u32, dir: PathBuf, bless: bool) -> Self {
        let mut files = BTreeMap::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let bytes = fs::read(entry.path()).expect("fixture reads");
                files.insert(name, bytes);
            }
        }
        Self {
            wire,
            dir,
            files,
            used: BTreeSet::new(),
            bless,
            written: Vec::new(),
            problems: Vec::new(),
        }
    }

    /// The fixture for `op` ending in `.ext`, matched by number so a renamed
    /// request keeps its fixtures.
    fn find(&self, op: u16, ext: &str) -> Option<String> {
        let prefix = format!("{op:03}-");
        let suffix = format!(".{ext}");
        self.files
            .keys()
            .find(|name| name.starts_with(&prefix) && name.ends_with(&suffix))
            .cloned()
    }

    fn write(&mut self, name: String, bytes: Vec<u8>) {
        fs::create_dir_all(&self.dir).expect("fixture dir");
        let path = self.dir.join(&name);
        assert!(!path.exists(), "{} exists", path.display());
        fs::write(&path, &bytes).expect("fixture writes");
        self.files.insert(name.clone(), bytes);
        self.used.insert(name.clone());
        self.written.push(name);
    }

    /// Checks the reply `name` against what the server sends now.
    fn check_reply(&mut self, name: &str, now: Vec<u8>) {
        self.used.insert(name.to_owned());
        if self.files[name] != now {
            self.problems.push(format!(
                "{}/{name}: the server's reply changed shape, and apps reading wire {} \
                 cannot decode it. Put the new shape behind a new request, or behind a \
                 `Feature` and `since` (see AGENTS.md).",
                self.wire, self.wire
            ));
        }
    }

    fn check_errors(&mut self) {
        let now = encode(&ApiError::samples(self.wire));
        if self.files.contains_key(ERRORS) {
            self.check_reply(ERRORS, now);
        } else if self.bless && self.wire == WIRE {
            self.write(ERRORS.to_owned(), now);
        } else {
            self.problems
                .push(format!("{}/{ERRORS} is missing", self.wire));
        }
    }

    fn check_leftovers(&mut self) {
        for name in self.files.keys().filter(|name| !self.used.contains(*name)) {
            self.problems.push(format!(
                "{}/{name} matches no request: requests are never removed and their \
                 numbers never reused",
                self.wire
            ));
        }
    }
}

impl Visit for Revision {
    fn request<R: Rpc + Sample, T: Serialize + Sample>(&mut self) {
        let op = R::OP as u16;
        let request = self.find(op, "request");
        let reply = self.find(op, "reply");
        let (request, reply) = match (request, reply) {
            (Some(request), Some(reply)) => (request, reply),
            // Newer than this revision, so none of its clients send it.
            (None, None) if self.wire < WIRE => return,
            (None, None) if self.bless => {
                let stem = format!("{op:03}-{:?}", R::OP);
                self.write(format!("{stem}.request"), encode(&R::samples(self.wire)));
                self.write(format!("{stem}.reply"), encode(&T::samples(self.wire)));
                return;
            }
            (None, None) => {
                self.problems.push(format!(
                    "{:?} has no fixtures in {}/: run `{BLESS}=1 cargo test -p beep-proto \
                     compat` and commit them",
                    R::OP,
                    self.wire
                ));
                return;
            }
            _ => {
                self.problems
                    .push(format!("{:?} lacks a request or reply fixture", R::OP));
                return;
            }
        };
        self.used.insert(request.clone());
        if let Err(e) = rmp_serde::from_slice::<Vec<R>>(&self.files[&request]) {
            self.problems.push(format!(
                "{}/{request}: apps reading wire {} send this, and it no longer decodes \
                 ({e}). Requests only gain trailing `#[serde(default)]` fields.",
                self.wire, self.wire
            ));
        }
        self.check_reply(&reply, encode(&T::samples(self.wire)));
    }
}

#[test]
fn released_apps_still_read_the_wire() {
    let bless = env::var_os(BLESS).is_some();
    let root = fixtures_dir();
    let mut wires: BTreeSet<u32> = fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .collect();
    let mut problems = Vec::new();
    if !wires.contains(&WIRE) {
        if bless {
            wires.insert(WIRE);
        } else {
            problems.push(format!(
                "tests/wire/{WIRE} is missing: run `{BLESS}=1 cargo test -p beep-proto compat`"
            ));
        }
    }
    for &wire in &wires {
        if wire > WIRE {
            problems.push(format!("tests/wire/{wire} is newer than WIRE ({WIRE})"));
            continue;
        }
        let mut revision = Revision::load(wire, root.join(wire.to_string()), bless);
        each_request(&mut revision);
        revision.check_errors();
        revision.check_leftovers();
        for name in &revision.written {
            println!("wrote tests/wire/{wire}/{name}");
        }
        problems.append(&mut revision.problems);
    }
    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
}

#[test]
fn old_apps_refuse_an_extra_element() {
    #[derive(Serialize)]
    struct Newer {
        a: u8,
        b: u8,
    }
    #[derive(serde::Deserialize, Debug)]
    struct Older {
        _a: u8,
    }
    let newer = encode(&Newer { a: 1, b: 2 });
    assert!(rmp_serde::from_slice::<Older>(&newer).is_err());
    // Inside a wire enum too: `[op, message, extra]`.
    let extra = encode(&(0u8, "x", true));
    assert!(rmp_serde::from_slice::<ApiError>(&extra).is_err());
}

#[test]
fn old_apps_refuse_an_unknown_op() {
    assert!(rmp_serde::from_slice::<ApiError>(&encode(&(9u8,))).is_err());
}
