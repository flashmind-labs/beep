import { flushSync } from "react-dom";
import { keepProjectPick, lastProjectPick, type ProjectPick } from "../agents/agentmodels.ts";
import { pickPopover } from "../agents/pick-popover.ts";
import type { AgentButtonItem, AgentState, PickButton } from "../review/feedback-view.tsx";
import type { SessionRow } from "./projects-state.ts";
import { syncRepairPrompt } from "./gitsync-state.ts";
import type { Picked } from "../agents/agentpicker.tsx";
import { StashBar } from "../chat/stash-hints.tsx";
import { ComposerChoices, type ComposerChoicesState, ComposerMachine } from "./composer-choices.tsx";
import { composeMessage } from "../chat/compose.tsx";
import { subscribeChanges, topic } from "../ui/mesh-events.ts";
import { AMBIENT_MS, latest, STALE } from "../ui/active.ts";
// The bar under a project page: its prompt, harness picker and checkout, and
// starting or resuming a coding agent from it.
import { makeStartComposer } from "../agents/start-composer.tsx";
import { menuPlaceFor } from "../agents/mentionmenu.ts";
import { sessionPicks, startedWith } from "../agents/session.tsx";
import { sentText } from "../agents/agentlog.ts";
import { reason } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import { commandKey, isMobile } from "../ui/platform.ts";
import { filterProjects } from "./projects-state.ts";
import { openMenu } from "../ui/menu.tsx";
import { localProjectId } from "./localid.ts";
import { addHosts, type BarPlace, type BarTarget, barCheckout, barPlace, barPrompt, barReadiness, blockedHosts, forgeOf, nextBarMachine, reviewPrompt, reviewSession, type ReviewSession, reviewTitle, sessionIn, targetCheckout, targetFolders, targetPlan } from "./projectbar.ts";
import { keepMachine, lastMachine, openAgents, openMachine, openMachines } from "../agents/agentnew.tsx";
import { TASK_WORDS } from "../agents/assistant-state.ts";
import type { Machine } from "../machines/machines.tsx";
import { AGENT_ORDER, type BaseBranches, baseFor, byPreference, type CheckoutPick, DEFAULT_CHECKOUT, harnessFor, keepEffort, knownWorktrees, startPlace } from "../agents/agentpick.ts";
import { BlockedHelp, NoAgentHelp } from "../agents/start-composer-view.tsx";
import { closedPicker, drawPicker, focusSearch, modelsOn, pillContent, preloadModels } from "../agents/agentpicker.tsx";
import { AG, type AgentHost, agentKind } from "../agents/agents.ts";
import { draftPlace, draftSent } from "../agents/draft.tsx";
import { S } from "../bar/core.ts";
import { ensureSessions, sessions } from "../agents/sessions.ts";
import { issueBranch, issuePrompt, type IssueRow, type PullRequestHead, type PullRequestRow } from "./threads.ts";
import { planStarts, type StartEntry, type StartRow } from "./openlist-state.ts";
import { fixPrompt, runTarget, type RunDetail } from "./runs.tsx";
import { conflictPrompt, type Conflict } from "./filesync.ts";
import { type Started, state } from "../center/state.ts";
import { currentLocation, go } from "../center/views.tsx";
import { projectRows } from "../center/sidebar.tsx";
import { redrawProjects, pa, pc } from "./sections.ts";
import { loadWhere, projectPlaces } from "./loads.tsx";
import { copyLine, shortName } from "./addmachine-state.ts";
import type { SyncDetail } from "./filesync.ts";
import { projectUnder } from "../addproject/importproject-state.ts";
import { projectsIn } from "../addproject/importproject.tsx";
import { refresh } from "../center/refresh.ts";
import { redrawSync } from "./syncpage.ts";
import { startAttachments } from "../agents/start-attachments.tsx";

let finishCopy: (() => void) | undefined;

export const projectComp = (() => {
  const composer = makeStartComposer({
    presentation: "compact", externalChoices: true, title: "Enter starts, Shift+Enter adds a line, Shift+Tab switches machines",
    actions: { submit: () => void startFromBar(), harness: toggleBarPicker, keyDown: switchMachine, files: files => barAttachments.take(files), images: files => barAttachments.add(files), removeLast: () => barAttachments.removeLast() },
    place: barMenuPlace,
  });
  const { form, harness, pick } = composer;
  const actions = {
    machine: openMachineMenu,
    folder: openFolderMenu,
    branch: (value: string) => { if (pc.target?.kind === "issue") pc.target.branch = value; },
    clear: () => { pc.target = null; composer.setText(""); redrawProjects(); },
  };
  const renderChoices = (value: ComposerChoicesState) => {
    composer.setMachine(<ComposerMachine value={value} open={actions.machine} />);
    composer.setChoices(<ComposerChoices value={value} actions={actions} />);
  };
  renderChoices({ machine: "Machine", on: false, adding: false, machineDisabled: true, folder: "Current checkout", folderSub: "as it is", folderDisabled: true, branch: "", showBranch: false, showClear: false });
  // Esc or a click outside closes the picker.
  form.addEventListener("keydown", e => {
    // ⇧⌘M opens the model picker, as on the agent start page. ⌘M minimizes on macOS.
    if (commandKey(e) && e.shiftKey && !e.altKey && e.key.toLowerCase() === "m") {
      e.preventDefault();
      e.stopPropagation();
      toggleBarPicker();
      return;
    }

    if (e.key !== "Escape" || !pc.open) return;
    e.preventDefault();
    e.stopPropagation();
    closeBarPicker();
    harness.focus();
  });
  addEventListener("mousedown", e => {
    if (pc.open && e.target instanceof Node && !pick.contains(e.target) && !harness.contains(e.target)) closeBarPicker();
  }, true);
  return { ...composer, renderChoices };
})();

/// Where the bar's `/` and `@` menus look: its machine's checkout, with the
/// commands of the harness picked there.
function barMenuPlace() {
  const p = barProject(), place = p && !pc.adding && barPlace(p.places, pc.hosts ?? [], pc.machine);
  return place && pc.picked.agent ? menuPlaceFor(place.machine_id, pc.picked.agent, place.cwd) : null;
}

/// Shift+Tab in the task field moves the bar to the project's next machine.
function switchMachine(e: KeyboardEvent) {
  if (e.key !== "Tab" || !e.shiftKey || e.altKey || e.ctrlKey || e.metaKey || e.isComposing || pc.adding || pc.starting) return;
  const p = barProject(), hosts = pc.hosts ?? [];
  const current = p && barPlace(p.places, hosts, pc.machine);
  const next = p && current && nextBarMachine(p.places, hosts, current.machine_id);
  if (!p || !next) return;

  e.preventDefault();
  pc.machine = next.machine_id;
  keepMachine(p.name, next.machine_id);
  redrawProjects();
  projectComp.input.focus();
}

/// The start form's picker, here above the bar.
function toggleBarPicker() {
  pc.open = !pc.open;
  pc.picked.query = "";
  redrawProjects();
}

function openMachineMenu(e: MouseEvent) {
  const p = barProject(), hosts = pc.hosts ?? [];
  if (!p || pc.adding) return;
  if (!addHosts(hosts).length) return openAgents();
  if (!barPlace(p.places, hosts, pc.machine)) {
    openMenu(e, "Add to a machine", addHosts(hosts).map(m => ({ label: m.machine_name, on: () => addToMachine(p.name, m) })));
    return;
  }
  const able = addHosts(hosts).filter(m => p.places.some(x => x.machine === m.machine_id));
  openMenu(e, "Machine", able.map(m => ({ label: m.machine_name, on: () => { pc.machine = m.machine_id; keepMachine(p.name, m.machine_id); redrawProjects(); } })));
}

function openFolderMenu(e: MouseEvent) {
  const p = barProject(), place = p && barPlace(p.places, pc.hosts ?? [], pc.machine);
  if (!place) return;
  const trees = knownWorktrees(sessions, place.machine_id, place.cwd);
  const pick = (to: CheckoutPick) => () => { pc.checkout = to; pc.target = null; redrawProjects(); };
  openMenu(e, "Folder", [
    { label: `Current checkout (${place.cwd.split("/").pop()})`, on: pick("current") },
    { label: "New worktree", on: pick("new") },
    { label: "New branch in the checkout", on: pick("branch") },
    ...trees.map(t => ({ label: `Worktree ${t.split("/").pop()}`, on: pick({ worktree: t }) })),
  ]);
}

const barAttachments = startAttachments(projectComp, error => projectComp.update({ error }), () => {
  const p = barProject();
  projectComp.update({ sendDisabled: !!pc.starting || barAttachments.reading || !p || !barPlace(p.places, pc.hosts ?? [], pc.machine) || !!pc.adding });
});

barAttachments.bindStash(() => pc.project);

function closeBarPicker() {
  pc.open = false;
  redrawProjects();
}

/// The machine the bar starts on (or is adding the project to, or would add
/// it to first), its harnesses, and the harness picked there, kept with the
/// model when the machine or harness changes.
function barHarness(place: { machine_id: string } | null) {
  const host = place ? pc.hosts?.find(m => m.machine_id === place.machine_id) ?? null : null;
  const offered = host ? [...host.agents].sort(byPreference) : [];
  const preferred = !pc.modelFor ? lastProjectPick(pc.project)?.agent : null;
  const agent = harnessFor(preferred ?? (pc.picked.agent || null), offered, AGENT_ORDER);
  if (agent && agent !== pc.picked.agent) pc.picked.effort = keepEffort(agent, pc.picked.effort);
  pc.picked.agent = agent ?? "";
  const key = host && agent ? `${host.machine_id}/${agent}` : "";
  if (pc.modelFor !== key) {
    const saved = !pc.modelFor ? lastProjectPick(pc.project) : null;
    pc.picked.model = saved?.agent === agent ? saved.model : "";
    pc.modelFor = key;
  }
  return { host, offered, agent };
}

/// The bar's project and its checkouts.
function barProject() {
  return pc.project ? projectOf(pc.project) : null;
}

/// Project `name` and its checkouts.
function projectOf(name: string) {
  const p = filterProjects(projectRows() || [], name)[0];
  return p ? { name: p.name, places: projectPlaces(p) } : null;
}

/// Whether the bar of project `name` is on screen: under its page, or in the
/// empty agent view.
export function barOn(name: string) {
  return (currentLocation()?.view === "projects" && state.picked === name) || (currentLocation()?.view === "pane" && S.view === "agent" && AG.draft === name);
}

// Initial and live reads share a generation so an older response cannot
// overwrite host availability reported by a newer machine event.
const hostsRead = latest();
async function readProjectHosts() {
  try {
    const read = await hostsRead.run(Promise.all([invoke<AgentHost[]>("agent_hosts"), invoke<Machine[] | null>("list_machines").catch((e: unknown) => { console.warn("machines for the bar:", e); return null; })]));
    if (read === STALE) return;
    const [list, machines] = read;
    pc.hosts = list;
    pc.blocked = blockedHosts(machines ?? [], list);
    preloadModels(list);
  } catch (error) {
    console.warn("agent hosts:", error);
    if (pc.hosts !== null) return;
    pc.hosts = [];
  }
  if (pc.project && barOn(pc.project)) redrawProjects();
}

// A host can return after the project opened during its update.
subscribeChanges([topic("machines"), topic("accounts")], { deliver: "latest", receive: () => readProjectHosts() }, { when: () => !!pc.project && barOn(pc.project), wait: AMBIENT_MS });

/// Reads what the bar needs for project `name`: the machines that run
/// agents, the sessions (for worktrees) and the project's branches.
function loadProjectBar(name: string) {
  const redraw = () => { if (barOn(name)) redrawProjects(); };
  void readProjectHosts();
  ensureSessions().then(redraw).catch(e => console.warn("agent sessions:", e));

  if (!pc.branches.has(name)) {
    invoke<BaseBranches | null>("project_branches", { project: name })
      .catch(() => null)
      .then(list => { pc.branches.set(name, list); redraw(); });
  }
}

/// The field fits its text again only when the text, its host or its layout
/// changed since it last did: fitting reads the field's height, which lays
/// the page out, and the page draws the bar on every open.
let fitted: { text: string; host: Element | null; centered: boolean } | null = null;

function fitField() {
  const c = projectComp, now = { text: c.input.value, host: c.form.parentElement, centered: c.centered() };
  if (fitted && fitted.text === now.text && fitted.host === now.host && fitted.centered === now.centered) return;
  fitted = now.host ? now : null;
  c.grow();
}

/// Points the bar at project `name`: its field, machine, folder and harness.
export function drawProjectComp(name: string) {
  const c = projectComp, inDraft = currentLocation()?.view === "pane" && S.view === "agent" && AG.draft === name;
  // The empty agent view shows the stash with its key hints.
  c.setFooter(inDraft ? null : <StashBar project={name} />);
  // A project with nothing on its board shows the bar as the empty agent view does.
  c.setPresentation(inDraft || pa.blank ? "centered" : "compact");

  if (pc.project !== name) {
    finishCopy?.();
    pc.project = name;
    pc.machine = lastMachine(name);
    const saved = lastProjectPick(name);
    pc.picked = { agent: saved?.agent ?? "", model: saved?.model ?? "", effort: saved?.effort ?? null, query: "" };
    pc.modelFor = "";
    pc.open = false;
    pc.adding = null;
    pc.checkout = DEFAULT_CHECKOUT;
    pc.target = null;
    c.update({ error: "" });
    loadProjectBar(name);
  }

  const p = barProject();
  // The machine last picked, while it still has a checkout and an agent,
  // else the first such machine.
  const place = p && barPlace(p.places, pc.hosts ?? [], pc.machine);
  const readiness = p ? barReadiness(p.places, pc.hosts, pc.machine, pc.blocked) : { kind: "ready" as const };
  // Text set or cleared from code (Work on it, a start) fits once the page places the form.
  requestAnimationFrame(fitField);
  const t = pc.target;
  const co = t ? targetCheckout(t) : barCheckout(pc.checkout, baseFor(null, pc.branches.get(name)));
  c.renderChoices({
    machine: pc.adding?.machine_name ?? (place ? place.machine_name : readiness.kind === "no agent" ? "Install a coding agent" : pc.hosts ? "Add to a machine" : "Machine"),
    on: !!place && !pc.adding, adding: !!pc.adding, machineDisabled: !!pc.adding || !!pc.starting,
    folder: co.label, folderSub: co.sub, folderDisabled: !place || !!pc.starting,
    branch: t?.kind === "issue" ? t.branch : "", showBranch: t?.kind === "issue" && !t.review?.earlier,
    showClear: !!t,
  });
  const { host, offered, agent } = barHarness(pc.adding ?? place ?? addHosts(pc.hosts ?? [])[0] ?? null);

  if (agent && host) {
    modelsOn(host.machine_id, agent, () => { if (barOn(name)) redrawProjects(); });
    c.setHarness(pillContent(agent, pc.picked.model, pc.picked.effort, host.machine_id));
  } else {
    c.setHarness("Coding agent");
  }

  if (!agent) pc.open = false;
  c.update({
    placeholder: c.centered() && readiness.kind === "ready" ? TASK_WORDS.placeholder : barPrompt(name, pc.adding ? { kind: "no checkout" } : readiness),
    project: name,
    harnessDisabled: !agent || !!pc.starting, open: pc.open ? "pick" : null,
    sendDisabled: !!pc.starting || barAttachments.reading || !place || !!pc.adding, busy: !!pc.starting,
    notes: readiness.kind === "no agent" ? <NoAgentHelp openAgents={openAgents} openMachines={openMachines} />
      : readiness.kind === "blocked" && !pc.adding ? <BlockedHelp host={readiness.host} open={() => openMachine(readiness.host.machine_id)} /> : null,
  });
  drawBarPicker(host, offered);
  barAttachments.setBusy(!!pc.starting);
  if (place && !pc.starting) barAttachments.files.moveTo(place.machine_id, place.cwd);
  draftPlace(place);
  draftSent(pc.starting);
}

/// The newest session of ours in the bar target's worktree on the bar's
/// machine, or null; none without a target.
function barExisting(name: string, place: { machine_id: string; cwd: string }, t: BarTarget | null) {
  if (!t || !t.branch.trim()) return null;
  // Where the branch list (read on one machine) found its worktree.
  const listed = pc.branches.get(name) as unknown as { machine_id?: string; branches: readonly { name: string; worktree?: string | null }[] } | null | undefined;
  const known = listed?.machine_id === place.machine_id ? listed.branches.find(b => b.name === t.branch.trim())?.worktree : null;
  return sessionIn(sessions, place.machine_id, targetFolders(t, place.cwd, known));
}

/// Fills the bar for project `name` with `text` and `target`, on the
/// machine last picked, and brings it into view.
export function fillBar(name: string, text: string, target: BarTarget | null) {
  if (pc.project !== name) drawProjectComp(name);
  pc.target = target;
  projectComp.setText(text);
  projectComp.update({ error: "" });
  // A session may have started in the worktree since.
  ensureSessions().then(redrawProjects).catch(e => console.warn("agent sessions:", e));
  redrawProjects();
  // A phone's project page has no bar: the empty agent view shows it.
  if (isMobile() && !projectComp.form.isConnected) return go("pane", { draft: { project: name, machine: null } });
  projectComp.form.scrollIntoView({ block: "nearest" });
  projectComp.input.focus();
}

/// Points the bar at project `name`, on `machine` when it has a checkout there.
export function pointBar(name: string, machine: string | null) {
  if (pc.project !== name) drawProjectComp(name);
  if (machine && barProject()?.places.some(p => p.machine === machine)) pc.machine = machine;
}

/// Work on it: the bar on a new worktree for the issue, from the default
/// branch, with the issue's prompt.
export function workOnIssue(name: string, issue: IssueRow) {
  const task = issueTask(name, issue);
  fillBar(name, task.prompt, task.target);
}

/// Work on it's task: a new worktree for the issue, from the default branch.
function issueTask(name: string, issue: IssueRow): AgentTask {
  const base = baseFor(null, pc.branches.get(name));
  return { prompt: issuePrompt(issue), target: { kind: "issue", number: issue.number, branch: issueBranch(issue.number, issue.title), base } };
}

/// Work on it on the issue's page: starts the agent at once.
export const startIssue = (name: string, issue: IssueRow, pick: ProjectPick) => startTask(name, issueTask(name, issue), pick);

/// Where each picked issue starts, on the machine and harness the bar has
/// picked (the one last picked for another project, and the first harness);
/// null until the machines are read.
export function issueStartPlan(entries: readonly StartEntry[]) {
  if (!pc.hosts) return null;
  const projects = projectRows() || [];
  const places = (name: string) => { const p = projects.find(x => x.name === name); return p ? projectPlaces(p) : []; };
  return planStarts(entries, places, pc.hosts, name => (pc.project === name ? pc.machine : lastMachine(name)), pc.picked.agent || null, AGENT_ORDER);
}

/// Reads the machines when no project's bar has.
export async function loadStartHosts() {
  pc.hosts ??= await invoke<AgentHost[]>("agent_hosts");
}

/// Starts one agent per row, one at a time in a new worktree each, waiting
/// for each to be accepted; stops at the first that fails. `done` runs after
/// each start with those started so far.
export async function startIssues(rows: readonly StartRow[], done: (started: StartRow[]) => void) {
  const started: StartRow[] = [];

  for (const row of rows) {
    if (row.skip !== null) continue;
    const { entry, agent, machine_id, cwd } = row;
    const same = agent === pc.picked.agent;
    const base = baseFor(null, pc.branches.get(entry.project!));
    const checkout = targetPlan({ kind: "issue", number: entry.number, branch: issueBranch(entry.number, entry.title), base });
    const model = same && pc.modelFor === `${machine_id}/${agent}` ? pc.picked.model.trim() || null : null;

    try {
      const effort = same ? pc.picked.effort : null;
      const session = await invoke<Started>("agent_start", { machineId: machine_id, agent, cwd, prompt: issuePrompt(entry), model, effort, checkout });
      if (model || effort) sessionPicks.set(session.session.id, { model: model ?? "", effort });
    } catch (e) {
      return { started, failed: row, error: reason(e) };
    }

    started.push(row);
    done(started);
  }

  return { started, failed: null, error: null };
}

/// Continue from the menu: fills the bar to start on the PR's branch.
export async function continuePr(name: string, pr: PullRequestRow) {
  const task = await continueTask(name, pr);
  fillBar(name, task.prompt, task.target);
}

/// Continue on the PR's page: starts the agent at once.
export async function startContinue(name: string, pr: PullRequestRow, pick: ProjectPick) {
  await startTask(name, await continueTask(name, pr), pick);
}

/// Continue's task: the PR's branch. A fork's branch is not on `origin`, so
/// it is refused.
async function continueTask(name: string, pr: PullRequestRow): Promise<AgentTask> {
  const head = await invoke<PullRequestHead>("center_pull_request_head", { projectId: localProjectId(name), number: pr.number });
  if (head.fork) throw new Error(`PR #${pr.number} comes from a fork, so its branch ${head.branch} is not in this repository.`);
  return { prompt: `Continue PR #${pr.number}: ${pr.title} (${pr.url}). Read the review comments and failing checks first.`, target: { kind: "pr", number: pr.number, branch: head.branch } };
}

/// The fields of a pull request a review reads.
type ReviewedPr = Pick<PullRequestRow, "number" | "title" | "url" | "mine">;

/// Review again from the menu: fills the bar with a review prompt, which
/// goes to the session that reviewed the PR before when there is one.
export async function reviewPr(name: string, pr: PullRequestRow) {
  const task = await reviewTask(name, pr);
  fillBar(name, task.prompt, task.target);
}

/// Review with agent on the PR's page: starts the reviewer at once.
export async function startReview(name: string, pr: ReviewedPr, pick: ProjectPick) {
  await startTask(name, await reviewTask(name, pr), pick);
}

/// A review of `pr`. A fork's branch is not on `origin`, so that review
/// starts on a new worktree from the default branch and reads the diff
/// through the forge CLI.
async function reviewTask(name: string, pr: ReviewedPr): Promise<AgentTask> {
  const head = await invoke<PullRequestHead>("center_pull_request_head", { projectId: localProjectId(name), number: pr.number });
  await ensureSessions().catch(e => console.warn("agent sessions:", e));
  const review = { number: pr.number, earlier: earlierReview(name, pr.number) };
  const target: BarTarget = head.fork
    ? { kind: "issue", number: pr.number, branch: `review-pr-${pr.number}`, base: baseFor(null, pc.branches.get(name)), review }
    : { kind: "pr", number: pr.number, branch: head.branch, review };
  return { prompt: reviewPrompt(pr, forgeOf(pr.url), head.fork, !!review.earlier), target };
}

/// Fix this from the menu: fills the bar with the fix.
export function fixRun(name: string, detail: RunDetail) {
  const task = fixTask(name, detail);
  fillBar(name, task.prompt, task.target);
}

/// Fix this on the run: starts the agent at once.
export function startFix(name: string, detail: RunDetail, pick: ProjectPick) {
  return startTask(name, fixTask(name, detail), pick);
}

/// Fix this's task: the failed run's branch (a new branch when it ran on the
/// default branch), with the failure and its log in the prompt.
function fixTask(name: string, detail: RunDetail): AgentTask {
  return { prompt: fixPrompt(detail), target: runTarget(detail.run, baseFor(null, pc.branches.get(name))) };
}

/// Fix with an agent from the menu: the project's bar filled to settle a
/// sync conflict, on a machine that has it. The user sends it.
export function fixConflict(name: string, c: Conflict) {
  if (!c.holder) return;
  go("projects", { project: name });
  fillBar(name, conflictPrompt(c), null);
  pc.machine = c.holder;
  redrawProjects();
}

/// Fix with an agent on a conflict: starts the agent at once in the
/// checkout of the machine that has it.
export function startConflictFix(name: string, c: Conflict, pick: ProjectPick) {
  return startTask(name, { prompt: conflictPrompt(c), target: null }, pick, c.holder);
}

/// Fix with an agent on a blocked Git sync: starts the agent at once in
/// `machine`'s checkout, with both checkouts and recent sessions in its prompt.
export async function startSyncRepair(name: string, machine: string, error: string, pick: ProjectPick) {
  const p = projectRows()?.find(p => p.name === name);
  if (!p) throw new Error(`${name} is not on a machine.`);
  const sessions = await invoke<SessionRow[]>("project_sessions", { project: name }).catch(e => { console.warn("repair sessions:", e); return []; });
  const prompt = syncRepairPrompt({ project: name, machine, error, checkouts: p.machines, git: state.gitsync.get(name) ?? [], sessions });
  await startTask(name, { prompt, target: null }, pick, machine);
}

/// The picker above the bar, when open.
function drawBarPicker(host: AgentHost | null, offered: string[]) {
  const el = projectComp.pick, p = pc.picked;
  if (!pc.open || !host || !p.agent) { closedPicker(el); return; }
  const focused = el.contains(document.activeElement);
  drawPicker(el, {
    render: view => { if (projectComp.form.isConnected) projectComp.setPicker(view); },
    picked: p,
    offered,
    everywhere: [...new Set((pc.hosts ?? []).flatMap(m => m.agents))],
    machine: { id: host.machine_id, name: host.machine_name },
    noMachine: "",
    anchor: () => projectComp.harness,
    bounds: () => projectComp.form.closest<HTMLElement>(".agconv") ?? projectComp.form.parentElement,
    changed: what => {
      if (what !== "models") {
        // A model found under another harness keeps with it.
        if (what === "model") pc.modelFor = `${host.machine_id}/${p.agent}`;
        p.effort = keepEffort(p.agent, p.effort);
        keepProjectPick(pc.project, p);
      }
      if (what === "model") { pc.open = false; redrawProjects(); return projectComp.input.focus(); }
      if (what !== "models") redrawProjects();
      else if (pc.project) drawProjectComp(pc.project);
      if (what === "agent") focusSearch(el);
    },
  });
  if (focused) focusSearch(el);
}

/// Starts a coding agent on the bar's task in the project's checkout, then
/// opens its session; Esc there comes back to the project.
async function startFromBar() {
  const c = projectComp, p = barProject(), text = composeMessage(c.input.value);
  const place = p && barPlace(p.places, pc.hosts ?? [], pc.machine);
  if (p && !place && !pc.adding && pc.hosts) { c.update({ error: barPrompt(p.name, barReadiness(p.places, pc.hosts, pc.machine, pc.blocked)) + "." }); return; }
  if (!p || !place || (!text && !barAttachments.count) || barAttachments.reading || pc.starting || pc.adding) return;
  const t = pc.target;
  if (t?.kind === "issue" && !t.branch.trim()) { c.update({ error: "Name the branch first." }); return; }
  if (t?.review?.earlier) return continueReview(t.review.number, t.review.earlier, text);
  const { agent } = barHarness(place);
  const images = barAttachments.images.map(({ name, mime, data }) => ({ name, mime, data }));
  c.update({ error: "" });
  pc.starting = { text: sentText(text, images), line: `Starting ${agent ? agentKind(agent).name : "the agent"} on ${place.machine_name}` };
  pc.open = false;
  redrawProjects();

  try {
    const prompt = await barAttachments.files.prompt(text, place.machine_id, place.cwd);
    if (barAttachments.files.pending.length) c.update({ progress: "Starting the agent…" });
    const session = await startSession(p.name, place, { ...pc.picked, agent }, prompt, images, t, pc.checkout);
    c.setText("");
    barAttachments.clear();
    pc.target = null;
    go("pane", { agent: session, adopt: true });
  } catch (e) {
    c.update({ error: reason(e) });
  } finally {
    c.update({ progress: "" });
    pc.starting = null;
    redrawProjects();
  }
}

type Image = { name: string; mime: string; data: string };

/// Starts a coding agent of project `name` on `place` with `prompt`: in
/// `target`'s worktree, beside a session already there, or without a target
/// in `checkout`. Names a review for its PR.
async function startSession(name: string, place: BarPlace, pick: Omit<ProjectPick, "agent"> & { agent: string | null }, prompt: string, images: Image[], target: BarTarget | null, checkout: CheckoutPick) {
  const was = barExisting(name, place, target);
  const model = pick.model.trim() || null, effort = pick.effort;
  const at = was ? { cwd: was.cwd, checkout: null }
    : target ? { cwd: place.cwd, checkout: targetPlan(target) }
    : startPlace(checkout, place.cwd, baseFor(null, pc.branches.get(name)));
  const started = await invoke<Started>("agent_start", { machineId: place.machine_id, agent: pick.agent, cwd: at.cwd, prompt, model, effort, checkout: at.checkout, images });
  if (model || effort) sessionPicks.set(started.session.id, { model: model ?? "", effort });
  startedWith(started.session, prompt, images);
  keepMachine(name, place.machine_id);
  if (target?.review) nameReview(started.session, target.review.number);

  return started.session;
}

/// What an agent button starts: the prompt and the worktree, or the
/// checkout as it is without one.
export interface AgentTask { prompt: string; target: BarTarget | null }

/// The machine agent buttons of project `name` start on: `machine` when it
/// can, else the one the bar would.
function threadPlace(name: string, machine: string | null) {
  const p = projectOf(name);
  machine ??= pc.project === name ? pc.machine : lastMachine(name);
  return p ? { place: barPlace(p.places, pc.hosts ?? [], machine), why: barPrompt(name, barReadiness(p.places, pc.hosts, machine, pc.blocked)) } : null;
}

/// Starts `task` of project `name` at once with `pick`, on `machine` when
/// it can, keeps the pick for the bar, and opens the session.
export async function startTask(name: string, task: AgentTask, pick: ProjectPick, machine: string | null = null) {
  const at = threadPlace(name, machine);
  if (!at?.place) throw new Error(`${at?.why ?? `${name} is not on a machine`}.`);
  keepProjectPick(name, pick);
  const session = await startSession(name, at.place, pick, task.prompt, [], task.target, DEFAULT_CHECKOUT);
  go("pane", { agent: session, adopt: true });
}

/// An agent button of project `name` (Continue, Review with agent, Work on
/// it, Fix this, Fix with an agent): the diff's picker under it, listing
/// `machine`'s harnesses when it can start there, whose model pick runs
/// `start`.
export function pickButton(name: string, start: (pick: ProjectPick) => void, machine: string | null = null): PickButton {
  const picked: Picked = { agent: "", model: "", effort: null, query: "" };
  const listeners = new Set<() => void>();
  let current: ReturnType<PickButton["snapshot"]> = { open: false, picker: null };
  const host = () => {
    const place = threadPlace(name, machine)?.place;
    return place ? pc.hosts?.find(h => h.machine_id === place.machine_id) ?? null : null;
  };
  const pop = pickPopover({
    picked,
    machine: () => { const h = host(); return h && { id: h.machine_id, name: h.machine_name, agents: [...h.agents].sort(byPreference) }; },
    everywhere: () => [...new Set((pc.hosts ?? []).flatMap(m => m.agents))],
    bounds: box => box.closest<HTMLElement>(".pj-det, .pj-actions"),
    opening: () => {
      const offered = host()?.agents ?? [];
      const saved = lastProjectPick(name);
      if (saved && offered.includes(saved.agent)) Object.assign(picked, saved);
      else if (!offered.includes(picked.agent)) Object.assign(picked, { agent: harnessFor(null, [...offered].sort(byPreference), AGENT_ORDER) ?? "", model: "", effort: null });
      return !!picked.agent;
    },
    start: () => start({ agent: picked.agent, model: picked.model, effort: picked.effort }),
    draw: sync => {
      current = { open: pop.open, picker: pop.view };
      const notify = () => { for (const listener of listeners) listener(); };
      if (sync) flushSync(notify); else notify();
    },
  });

  return {
    snapshot: () => current,
    subscribe: listener => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    setBox: pop.setBox,
    setButton: pop.setButton,
    setSlot: pop.setSlot,
    // Without a bar, as on a phone, the machines are read on the first click.
    click: () => void (pc.hosts ? pop.toggle() : readProjectHosts().then(() => pop.toggle())),
    why: () => { const at = threadPlace(name, machine); return !pc.hosts ? "" : at?.place ? "" : `${at?.why ?? `${name} is not on a machine`}.`; },
  };
}

/// The agent buttons of project pages: Fix this, and Fix with an agent in sync.
export const projectButtons = agentButtons(() => { redrawProjects(); redrawSync(); });

/// What one agent button starts, as its owner draws it now.
export interface AgentButtonOpts {
  project: string;
  /// The machine it starts on when it can; null for the bar's.
  machine: string | null;
  label: string;
  hint: string;
  /// The session it started, still working, which the button opens.
  running: { agent: string; open(): void } | null;
  start: (pick: ProjectPick) => Promise<void>;
}

/// Agent buttons kept by key across draws, with the starts going on and why
/// each last failed. A pick runs the `start` of the latest draw.
export function agentButtons(redraw: () => void) {
  const made = new Map<string, PickButton>();
  const latest = new Map<string, AgentButtonOpts>();
  const starting = new Set<string>();
  const failed = new Map<string, string>();

  async function run(key: string, pick: ProjectPick) {
    const o = latest.get(key);
    if (!o || starting.has(key)) return;
    starting.add(key);
    failed.delete(key);
    redraw();

    try {
      await o.start(pick);
    } catch (e) {
      failed.set(key, reason(e));
    } finally {
      starting.delete(key);
      redraw();
    }
  }

  return (key: string, o: AgentButtonOpts): AgentButtonItem => {
    latest.set(key, o);
    let button = made.get(key);
    if (!button) made.set(key, button = pickButton(o.project, pick => void run(key, pick), o.machine));
    const state: AgentState = o.running ? { kind: "running", agent: o.running.agent, hint: "Open the session", open: o.running.open }
      : starting.has(key) ? { kind: "starting" } : { kind: "ready", label: o.label, hint: o.hint };

    return { button, state, error: failed.get(key) ?? "" };
  };
}


/// The session that last reviewed PR `number` of `project`, found by its
/// name among the project's sessions on every machine, as it is now.
export function earlierReview(project: string, number: number): ReviewSession | null {
  const found = reviewSession(state.sessions.get(project) ?? [], number);
  return found && (sameSession(found) ?? found);
}

const sameSession = (s: ReviewSession) => sessions.find(l => l.machine_id === s.machine_id && l.agent === s.agent && l.id === s.id) ?? null;

/// Names a review session for its PR, which finds it for the next review.
function nameReview(s: ReviewSession, number: number) {
  invoke("agent_title_save", { machineId: s.machine_id, agent: s.agent, session: s.id, title: reviewTitle(number) })
    .catch(e => console.warn("review title:", e));
}

/// Sends the review to the session that reviewed the PR before, resuming it
/// when it ended, and opens it.
async function continueReview(number: number, earlier: ReviewSession, text: string) {
  const c = projectComp;
  const listed = sameSession(earlier);
  const at = { machineId: earlier.machine_id, agent: earlier.agent, session: earlier.id };
  const images = barAttachments.images.map(({ name, mime, data }) => ({ name, mime, data }));
  c.update({ error: "" });
  pc.starting = { text: sentText(text, images), line: `Sending to ${agentKind(earlier.agent).name} on ${earlier.machine_name}` };
  redrawProjects();

  try {
    const root = listed?.cwd ?? barProject()?.places.find(p => p.machine === earlier.machine_id)?.cwd ?? "";
    const prompt = await barAttachments.files.prompt(text, earlier.machine_id, root);
    let session = listed;

    if (!session || session.status === "ended") {
      session = (await invoke<Started>("agent_resume", { ...at, prompt: "" })).session;
      if (session.id !== earlier.id) nameReview(session, number);
    }

    await invoke("agent_steer", { machineId: session.machine_id, agent: session.agent, session: session.id, steer: { prompt: { text: prompt, images } } });
    barAttachments.clear();
    c.setText("");
    pc.target = null;
    go("pane", { agent: session });
  } catch (e) {
    c.update({ error: reason(e) });
  } finally {
    pc.starting = null;
    redrawProjects();
  }
}

/// Adds `project` to machine `m` as its machine row's Add does, in the folder
/// that sheet offers first, with the copy's progress on the bar's line under
/// it; then the bar is ready there.
async function addToMachine(project: string, m: AgentHost) {
  const say = (error: string) => projectComp.update({ error }), here = () => pc.project === project;
  pc.adding = m;
  pc.machine = m.machine_id;
  keepMachine(project, m.machine_id);
  say(`Adding ${shortName(project)} to ${m.machine_name}…`);
  redrawProjects();

  try {
    const folder = projectUnder(await projectsIn(m.machine_id), project);
    await invoke<string>("project_add_to_machine", { project, machineId: m.machine_id, folder });
    refresh();
    loadWhere(project);
    await new Promise<void>(resolve => {
      let reading = false, again = false, done = false;
      const finish = () => { done = true; unfollow(); if (finishCopy === finish) finishCopy = undefined; resolve(); };
      const read = async () => {
        if (reading) { again = true; return; }
        reading = true;

        do {
          again = false;
          if (done) break;
          if (!here()) { finish(); break; }
          const detail = await invoke<SyncDetail | null>("project_sync_detail", { project }).catch(() => null);
          if (done || !here()) { finish(); break; }
          const line = copyLine(detail, m.machine_id, m.machine_name);
          if (line.done) { finish(); break; }
          say(line.note ? `${line.text}. ${line.note}` : line.text);
        } while (again && !done);

        reading = false;
      };
      const unfollow = subscribeChanges([topic("sync"), topic("projects")], { deliver: "latest", receive: () => read() });
      finishCopy = finish;
      void read();
    });
    if (here()) say("");
  } catch (e) {
    if (here()) say(reason(e));
  } finally {
    if (pc.adding === m) pc.adding = null;
    refresh();
    redrawProjects();
  }
}
