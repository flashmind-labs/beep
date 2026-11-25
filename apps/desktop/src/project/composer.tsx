import { flushSync } from "react-dom";
import { keepProjectAccount, keepProjectPick, lastProjectPick, type ProjectPick, startAccount } from "../agents/agentmodels.ts";
import { pickPopover } from "../agents/pick-popover.ts";
import { reopened } from "../agents/closing.ts";
import type { AgentButtonItem, AgentState, PickButton } from "../review/feedback-view.tsx";
import { agentLabel } from "./projects-state.ts";
import { syncRepairPrompt } from "./gitsync-state.ts";
import type { Picked } from "../agents/agentpicker.tsx";
import { StashBar } from "../chat/stash-hints.tsx";
import { ComposerChoices, type ComposerChoicesState, ComposerMachine } from "./composer-choices.tsx";
import { composeMessage } from "../chat/compose.tsx";
import { subscribeChanges, subscribeEach, topic } from "../ui/mesh-events.ts";
import { pushedView } from "../ui/snapshot-state.ts";
import { AMBIENT_MS } from "../ui/active.ts";
// The bar under a project page: its prompt, harness picker and checkout, and
// starting or resuming a coding agent from it.
import { makeStartComposer } from "../agents/start-composer.tsx";
import { menuPlaceFor } from "../agents/mentionmenu.ts";
import { sessionPicks, startedWith } from "../agents/session.tsx";
import { reason } from "../ui/format.ts";
import { invoke, listen } from "../ui/tauri.ts";
import type { AgentStarted } from "../agents/starting-state.ts";
import { commandKey, isMobile } from "../ui/platform.ts";
import { filterProjects } from "./projects-state.ts";
import { openMenu } from "../ui/menu.tsx";
import { localProjectId } from "./localid.ts";
import { addHosts, type BarPlace, type BarTarget, barCheckout, barPlace, barPrompt, barReadiness, blockedHosts, forgeOf, nextBarMachine, reviewPrompt, reviewTitled, type SessionTask, sessionIn, type TaskSession, taskSession, taskTitle, targetCheckout, targetFolders, targetPlan } from "./projectbar.ts";
import { keepMachine, lastMachine, openAgents, openMachine, openMachines } from "../agents/agentnew.tsx";
import { TASK_WORDS } from "../agents/assistant-state.ts";
import { AGENT_ORDER, allowedOf, type BaseBranches, baseFor, byPreference, type CheckoutPick, DEFAULT_CHECKOUT, harnessFor, keepEffort, knownWorktrees, machineRows, type MachineRow, startPlace } from "../agents/agentpick.ts";
import { BlockedHelp, NoAgentHelp } from "../agents/start-composer-view.tsx";
import { closedPicker, drawPicker, focusSearch, pillContent } from "../agents/agentpicker.tsx";
import { modelsOn } from "../agents/model-store.ts";
import { projectHarnesses, readProjectHarnesses } from "./project-harnesses.tsx";
import { AG, accountsOf, type AgentHost, ensureAgentHosts, loadAccountNames } from "../agents/agents-state.ts";
import { agentKind } from "../agents/agent-kind.ts";
import { ensureMachines, MACHINES } from "../machines/machine-store.ts";
import { draftPlace } from "../agents/draft.tsx";
import { S } from "../bar/core.ts";
import { ensureSessions, sessions } from "../agents/sessions.ts";
import { AGENT_ACTIVE } from "../agents/sessionmenu-state.ts";
import { issueBranch, issuePrompt, type DiscussionTopic, type IssueRow, type PullRequestHead, type PullRequestRow } from "./threads.ts";
import { planStarts, type StartEntry, type StartRow } from "./openlist-state.ts";
import type { Outcome } from "./openlist.ts";
import { fixPrompt, runTarget, type RunDetail } from "./runs.tsx";
import { conflictPrompt, type Conflict } from "./filesync.ts";
import { type Started, state, sessionsOf, gitOf } from "../center/state.ts";
import { currentLocation, go } from "../center/nav.ts";
import { projectRows } from "./projectrows.ts";
import { redraw, redrawSync } from "../center/redraw.ts";
import { pa, pc } from "./barstate.ts";
import { loadSessions, projectPlaces } from "./loads.tsx";
import { copyLine, shortName } from "./addmachine-state.ts";
import type { SyncDetail } from "./filesync.ts";
import { projectUnder } from "../addproject/importproject-state.ts";
import { projectsIn } from "../addproject/importproject.tsx";
import { startAttachments } from "../agents/start-attachments.tsx";
import { heldBy, takeFree, takeIssue } from "./team-view.tsx";

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
    account: openAccountMenu,
    branch: (value: string) => { if (pc.target?.kind === "issue") pc.target.branch = value; },
    clear: () => { pc.target = null; composer.setText(""); redraw("projects"); },
  };
  const renderChoices = (value: ComposerChoicesState) => {
    composer.setMachine(<ComposerMachine value={value} open={actions.machine} />);
    composer.setChoices(<ComposerChoices value={value} actions={actions} />);
  };
  renderChoices({ machine: "Machine", on: false, adding: false, machineDisabled: true, folder: "Current checkout", folderSub: "as it is", folderDisabled: true, branch: "", showBranch: false, showClear: false, account: null });
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
  return Object.assign(composer, { renderChoices });
})();

/// Where the bar's `/` and `@` menus look: its machine's checkout, with the
/// commands of the harness picked there.
function barMenuPlace() {
  const p = barProject(), place = p && !pc.adding && barPlace(p.places, AG.hosts ?? [], pc.machine);
  return place && pc.picked.agent ? menuPlaceFor(place.machine_id, pc.picked.agent, place.cwd) : null;
}

/// Shift+Tab in the task field moves the bar to the project's next machine.
function switchMachine(e: KeyboardEvent) {
  if (e.key !== "Tab" || !e.shiftKey || e.altKey || e.ctrlKey || e.metaKey || e.isComposing || pc.adding || pc.starting) return;
  const p = barProject(), hosts = AG.hosts ?? [];
  const current = p && barPlace(p.places, hosts, pc.machine);
  const next = p && current && nextBarMachine(p.places, hosts, current.machine_id);
  if (!p || !next) return;

  e.preventDefault();
  pc.machine = next.machine_id;
  keepMachine(p.name, next.machine_id);
  redraw("projects");
  projectComp.input.focus();
}

/// The start form's picker, here above the bar.
function toggleBarPicker() {
  pc.open = !pc.open;
  pc.picked.query = "";
  redraw("projects");
  if (!pc.open) projectComp.input.focus();
}

function openMachineMenu(e: MouseEvent) {
  const p = barProject(), hosts = AG.hosts ?? [];
  if (!p || pc.adding) return;
  if (!addHosts(hosts).length) return openAgents();
  if (!barPlace(p.places, hosts, pc.machine)) {
    openMenu(e, "Add to a machine", addHosts(hosts).map(m => ({ label: m.machine_name, on: () => addToMachine(p.name, m) })));
    return;
  }
  const able = addHosts(hosts).filter(m => p.places.some(x => x.machine === m.machine_id));
  openMenu(e, "Machine", able.map(m => ({ label: m.machine_name, on: () => { pc.machine = m.machine_id; keepMachine(p.name, m.machine_id); redraw("projects"); } })));
}

function openFolderMenu(e: MouseEvent) {
  const p = barProject(), place = p && barPlace(p.places, AG.hosts ?? [], pc.machine);
  if (!place) return;
  const trees = knownWorktrees(sessions, place.machine_id, place.cwd);
  const pick = (to: CheckoutPick) => () => { pc.checkout = to; pc.target = null; redraw("projects"); };
  openMenu(e, "Folder", [
    { label: `Current checkout (${place.cwd.split("/").pop()})`, on: pick("current") },
    { label: "New worktree", on: pick("new") },
    { label: "New branch in the checkout", on: pick("branch") },
    ...trees.map(t => ({ label: `Worktree ${t.split("/").pop()}`, on: pick({ worktree: t }) })),
  ]);
}

/// The bar's agent's accounts: the default one, then its named ones.
function openAccountMenu(e: MouseEvent) {
  const p = barProject(), agent = barAgent();
  if (!p || !agent) return;
  const names = accountsOf(agent), picked = startAccount(names, p.name, agent);
  const line = (name: string | null) => ({ kind: "toggle", label: name ?? "Default", checked: picked === name, on: () => { keepProjectAccount(p.name, agent, name); redraw("projects"); } }) as const;
  openMenu(e, "Account", [line(null), ...names.map(line)]);
}

/// The harness the bar starts, when it has one.
function barAgent() {
  const p = barProject(), place = p && barPlace(p.places, AG.hosts ?? [], pc.machine);
  return barHarness(pc.adding ?? place ?? addHosts(AG.hosts ?? [])[0] ?? null).agent;
}

const barAttachments = startAttachments(projectComp, error => projectComp.update({ error }), () => {
  const p = barProject();
  projectComp.update({ sendDisabled: pc.starting || barAttachments.reading || !p || !barPlace(p.places, AG.hosts ?? [], pc.machine) || !!pc.adding });
});

function closeBarPicker() {
  pc.open = false;
  redraw("projects");
}

/// The machine the bar starts on (or is adding the project to, or would add
/// it to first), its harnesses, and the harness picked there, kept with the
/// model when the machine or harness changes.
function barHarness(place: { machine_id: string } | null) {
  const host = place ? AG.hosts?.find(m => m.machine_id === place.machine_id) ?? null : null;
  const offered = host ? allowedOf(host.agents, projectHarnesses(pc.project)).sort(byPreference) : [];
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

/// The machines that have a coding agent that no host runs yet, from the machines and hosts held now.
const blockedNow = () => blockedHosts(MACHINES.list ?? [], AG.hosts ?? []);

// The hosts and machines are held from their pushes; the first read fills them. The held values
// are read after it, so a push that came meanwhile is not undone.
async function readProjectHosts() {
  try {
    await Promise.all([ensureAgentHosts(), ensureMachines().catch((e: unknown) => console.warn("machines for the bar:", e))]);
  } catch (error) {
    console.warn("agent hosts:", error);
  }
  if (pc.project && barOn(pc.project)) redraw("projects");
}

// A host can return after the project opened during its update. Followed
// while the bar is hidden too: drawing the same project again does not read
// the hosts, so a machine that reported meanwhile would stay missing.
subscribeChanges([topic("machines"), topic("accounts")], { deliver: "latest", receive: () => readProjectHosts() }, { when: () => !!pc.project, wait: AMBIENT_MS });

/// Reads what the bar needs for project `name`: the machines that run
/// agents, the sessions (for worktrees) and the project's branches.
function loadProjectBar(name: string) {
  const redrawBar = () => { if (barOn(name)) redraw("projects"); };
  void readProjectHosts();
  void readProjectHarnesses(name).then(redrawBar);
  ensureSessions().then(redrawBar).catch(e => console.warn("agent sessions:", e));

  if (!pc.branches.has(name)) {
    invoke<BaseBranches | null>("project_branches", { project: name })
      .catch(() => null)
      .then(list => { pc.branches.set(name, list); redrawBar(); });
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
    c.input.blur();
    finishCopy?.();
    pc.project = name;
    pc.machine = lastMachine(name);
    const saved = lastProjectPick(name);
    pc.picked = { agent: saved?.agent ?? "", model: saved?.model ?? "", effort: saved?.effort ?? null, fast: saved?.fast ?? null, query: "" };
    pc.modelFor = "";
    pc.open = false;
    pc.adding = null;
    pc.checkout = DEFAULT_CHECKOUT;
    pc.target = null;
    // Each project keeps its own unsent task.
    c.update({ error: "", draft: `project:${name}` });
    barAttachments.bindStash(() => name);
    fitted = null;
    loadProjectBar(name);
    void loadAccountNames().then(() => { if (barOn(name)) redraw("projects"); });
  }

  const p = barProject();
  // The machine last picked, while it still has a checkout and an agent,
  // else the first such machine.
  const place = p && barPlace(p.places, AG.hosts ?? [], pc.machine);
  const readiness = p ? barReadiness(p.places, AG.hosts, pc.machine, blockedNow()) : { kind: "ready" as const };
  // Text set or cleared from code (Work on it, a start) fits once the page places the form.
  requestAnimationFrame(fitField);
  const t = pc.target;
  const co = t ? targetCheckout(t) : barCheckout(pc.checkout, baseFor(null, pc.branches.get(name)));
  const { host, offered, agent } = barHarness(pc.adding ?? place ?? addHosts(AG.hosts ?? [])[0] ?? null);
  const names = accountsOf(agent);
  c.renderChoices({
    machine: pc.adding?.machine_name ?? (place ? place.machine_name : readiness.kind === "no agent" ? "Install a coding agent" : AG.hosts ? "Add to a machine" : "Machine"),
    on: !!place && !pc.adding, adding: !!pc.adding, machineDisabled: !!pc.adding || pc.starting,
    folder: co.label, folderSub: co.sub, folderDisabled: !place || pc.starting,
    branch: t?.kind === "issue" ? t.branch : "", showBranch: t?.kind === "issue",
    showClear: !!t,
    account: names.length ? { name: startAccount(names, name, agent) } : null,
  });

  if (agent && host) {
    modelsOn(host.machine_id, agent, () => { if (barOn(name)) redraw("projects"); });
    c.setHarness(pillContent(agent, pc.picked.model, pc.picked.effort, host.machine_id));
  } else {
    c.setHarness("Coding agent");
  }

  if (!agent) pc.open = false;
  c.update({
    placeholder: c.centered() && readiness.kind === "ready" ? TASK_WORDS.placeholder : barPrompt(name, pc.adding ? { kind: "no checkout" } : readiness),
    project: name,
    harnessDisabled: !agent || pc.starting, open: pc.open ? "pick" : null,
    sendDisabled: pc.starting || barAttachments.reading || !place || !!pc.adding, busy: pc.starting,
    notes: readiness.kind === "no agent" ? <NoAgentHelp openAgents={openAgents} openMachines={openMachines} />
      : readiness.kind === "blocked" && !pc.adding ? <BlockedHelp host={readiness.host} open={() => openMachine(readiness.host.machine_id)} /> : null,
  });
  drawBarPicker(host, offered);
  barAttachments.setBusy(pc.starting);
  if (place && !pc.starting) barAttachments.files.moveTo(place.machine_id, place.cwd);
  draftPlace(place);
}

/// The newest session of ours in the bar target's worktree on the bar's
/// machine, or null; none without a target.
function barExisting(name: string, place: { machine_id: string; cwd: string }, t: BarTarget | null) {
  if (!t || t.kind === "review" || !t.branch.trim()) return null;
  return sessionIn(sessions, place.machine_id, targetFolders(t, place.cwd, knownWorktree(name, place.machine_id, t)));
}

/// Where the branch list of project `name`, read on one machine, found the
/// worktree of `t`'s branch when that machine is `machine`.
function knownWorktree(name: string, machine: string, t: { branch: string }) {
  const listed = pc.branches.get(name) as unknown as { machine_id?: string; branches: readonly { name: string; worktree?: string | null }[] } | null | undefined;
  return listed?.machine_id === machine ? listed.branches.find(b => b.name === t.branch.trim())?.worktree : null;
}

/// Fills the bar for project `name` with `text` and `target`, on the
/// machine last picked, and brings it into view.
export function fillBar(name: string, text: string, target: BarTarget | null) {
  if (pc.project !== name) drawProjectComp(name);
  pc.target = target;
  projectComp.setText(text);
  projectComp.update({ error: "" });
  // A session may have started in the worktree since.
  ensureSessions().then(() => redraw("projects")).catch(e => console.warn("agent sessions:", e));
  redraw("projects");
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

/// What working on an issue needs of it.
type IssueStart = Pick<IssueRow, "number" | "title" | "url">;

/// Work on it: the bar on a new worktree for the issue, from the default
/// branch, with the issue's prompt.
export function workOnIssue(name: string, issue: IssueStart) {
  const task = issueTask(name, issue);
  takeIssue(name, issue, () => fillBar(name, task.prompt, task.target)).catch(error => projectComp.update({ error: reason(error) }));
}

/// Work on it's task: a new worktree for the issue, from the default branch.
function issueTask(name: string, issue: IssueStart): AgentTask {
  const base = baseFor(null, pc.branches.get(name));
  return { prompt: issuePrompt(issue), target: { kind: "issue", number: issue.number, branch: issueBranch(issue.number, issue.title), base, task: { kind: "issue", number: issue.number } } };
}

/// The session working on `issue` of project `name`, or null.
export const issueSession = (name: string, issue: IssueRow) => sessionFor(name, { kind: "issue", number: issue.number }, issueTask(name, issue).target);

/// The session fixing the failed run of `detail`, or null.
export const fixSession = (name: string, detail: RunDetail) => sessionFor(name, { kind: "run", workflow: detail.run.workflow, number: detail.run.number }, runTarget(detail.run, pc.branches.get(name)));

/// Work on it on the issue's page: starts the agent at once.
export const startIssue = (name: string, issue: IssueRow, pick: ProjectPick, machine: string | null) => takeIssue(name, issue, () => startTask(name, issueTask(name, issue), pick, machine));

/// Prefills the project composer with a discussion prompt.
export function discussThread(name: string, topic: DiscussionTopic) {
  const subject = topic.kind === "issue" ? "issue" : "pull request";
  const prompt = `I want to discuss about ${subject} #${topic.number}: ${topic.title} (${topic.url}).\n\n`;
  fillBar(name, prompt, null);
  projectComp.input.setSelectionRange(prompt.length, prompt.length);
}

/// Where each picked issue starts, on the machine and harness the bar has
/// picked (the one last picked for another project, and the first harness);
/// null until the machines are read.
export function issueStartPlan(entries: readonly StartEntry[]) {
  if (!AG.hosts) return null;
  const projects = projectRows() || [];
  const places = (name: string) => { const p = projects.find(x => x.name === name); return p ? projectPlaces(p) : []; };
  const rows = planStarts(entries, places, AG.hosts, name => (pc.project === name ? pc.machine : lastMachine(name)), pc.picked.agent || null, AGENT_ORDER);
  // An issue a teammate has is left out: its own page asks them first.
  return rows.map((row): StartRow => {
    const held = row.skip === null && row.entry.project ? heldBy(row.entry.project, row.entry.number) : null;
    return held ? { entry: row.entry, skip: held } : row;
  });
}

/// Reads the machines when no project's bar has.
export async function loadStartHosts() {
  await ensureAgentHosts();
}

/// Starts one agent per row, one at a time in a new worktree each, waiting
/// for each to be accepted; stops at the first that fails. A row found held
/// by a teammate only now is left out too. `done` runs after each start with
/// those started so far.
export async function startIssues(rows: readonly StartRow[], done: (started: StartRow[]) => void): Promise<Outcome> {
  const started: StartRow[] = [], left = rows.filter(row => row.skip !== null);

  for (const row of rows) {
    if (row.skip !== null) continue;
    const { entry, agent, machine_id, cwd } = row;
    const same = agent === pc.picked.agent;
    const base = baseFor(null, pc.branches.get(entry.project!));
    const task: SessionTask = { kind: "issue", number: entry.number };
    const checkout = targetPlan({ kind: "issue", number: entry.number, branch: issueBranch(entry.number, entry.title), base, task });
    const model = same && pc.modelFor === `${machine_id}/${agent}` ? pc.picked.model.trim() || null : null;

    try {
      const effort = same ? pc.picked.effort : null;
      const fast = same && agent === "codex" ? pc.picked.fast : null;
      const held = entry.project ? await takeFree(entry.project, entry.number) : null;
      if (held !== null) {
        left.push({ entry, skip: held });
        continue;
      }
      const prompt = issuePrompt(entry);
      await loadAccountNames();
      const account = startAccount(accountsOf(agent), entry.project ?? null, agent);
      const session = await invoke<Started>("agent_start", { machineId: machine_id, agent, cwd, prompt, model, effort, fast, checkout, images: [], account });
      if (model || effort || fast !== null) sessionPicks.set(session.session.id, { model: model ?? "", effort, fast });
      startedWith(session.session, prompt, []);
      nameSession(session.session, task);
    } catch (e) {
      return { started, failed: row, error: reason(e), left };
    }

    started.push(row);
    done(started);
  }

  return { started, failed: null, error: null, left };
}

/// Work on PR from the menu fills the bar to start on its branch.
export async function workOnPr(name: string, pr: PullRequestRow) {
  const task = await prTask(name, pr);
  fillBar(name, task.prompt, task.target);
}

/// Work on PR starts a session on the PR's branch on the chosen machine.
export async function startPrWork(name: string, pr: PrForWork, pick: ProjectPick, machine: string | null) {
  await startTask(name, await prTask(name, pr), pick, machine);
}

/// The fields needed to start work on a pull request.
type PrForWork = Pick<PullRequestRow, "number" | "title" | "url">;

/// The task targets the PR's branch. A fork's branch is not on `origin`, so
/// it is refused.
async function prTask(name: string, pr: PrForWork) {
  const head = await invoke<PullRequestHead>("center_pull_request_head", { projectId: localProjectId(name), number: pr.number });
  if (head.fork) throw new Error(`PR #${pr.number} comes from a fork, so its branch ${head.branch} is not in this repository.`);
  const target: BarTarget = { kind: "pr", number: pr.number, branch: head.branch, task: null };
  return { prompt: `Work on PR #${pr.number}: ${pr.title} (${pr.url}). Read the review comments and failing checks first.`, target };
}

/// The fields of a pull request a review reads.
type ReviewedPr = Pick<PullRequestRow, "number" | "title" | "url" | "mine">;

/// Review again from the menu: fills the bar with a review prompt, which
/// goes to the session that reviewed the PR before when there is one.
export async function reviewPr(name: string, pr: PullRequestRow) {
  const task = await reviewTask(name, pr);
  fillBar(name, task.prompt, task.target);
}

/// Review with agent on the PR's page and on Home: starts the reviewer at
/// once, on `machine` when it can, or reviews again in the session that
/// reviewed the PR before.
export async function startReview(name: string, pr: ReviewedPr, pick: ProjectPick, machine: string | null = null) {
  const task = await reviewTask(name, pr);
  const { earlier } = task.target;
  if (earlier) return steerReview(pr.number, earlier, task.prompt, []);
  await startTask(name, task, pick, machine);
}

/// A review of `pr` reads the forge diff and inspects the PR head by ref.
async function reviewTask(name: string, pr: ReviewedPr) {
  await ensureSessions().catch(e => console.warn("agent sessions:", e));
  const earlier = earlierReview(name, pr.number);
  const target: BarTarget = { kind: "review", task: { kind: "review", number: pr.number }, earlier };
  return { prompt: reviewPrompt(pr, forgeOf(pr.url), earlier !== null), target };
}

/// Fix this from the menu: fills the bar with the fix.
export function fixRun(name: string, detail: RunDetail) {
  const task = fixTask(name, detail);
  fillBar(name, task.prompt, task.target);
}

/// Fix this on the run: starts the agent at once.
export function startFix(name: string, detail: RunDetail, pick: ProjectPick, machine: string | null) {
  return startTask(name, fixTask(name, detail), pick, machine);
}

/// Fix this's task: the failed run's branch (a new branch when it ran on the
/// default branch), with the failure and its log in the prompt.
function fixTask(name: string, detail: RunDetail): AgentTask {
  return { prompt: fixPrompt(detail), target: runTarget(detail.run, pc.branches.get(name)) };
}

/// Fix with an agent from the menu: the project's bar filled to settle a
/// sync conflict, on a machine that has it. The user sends it.
export function fixConflict(name: string, c: Conflict) {
  if (!c.holder) return;
  go("projects", { project: name });
  fillBar(name, conflictPrompt(c), null);
  pc.machine = c.holder;
  redraw("projects");
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
  await loadSessions(name);
  const sessions = sessionsOf(name) ?? [];
  const prompt = syncRepairPrompt({ project: name, machine, error, checkouts: p.machines, git: gitOf(name) ?? [], sessions });
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
    everywhere: [...new Set((AG.hosts ?? []).flatMap(m => m.agents))],
    allowed: projectHarnesses(pc.project),
    machine: { id: host.machine_id, name: host.machine_name },
    noMachine: "",
    steps: null,
    anchor: () => projectComp.harness,
    changed: what => {
      if (what !== "models") {
        // A model found under another harness keeps with it.
        if (what === "model") pc.modelFor = `${host.machine_id}/${p.agent}`;
        p.effort = keepEffort(p.agent, p.effort);
        keepProjectPick(pc.project, p);
      }
      if (what === "model") { pc.open = false; redraw("projects"); return projectComp.input.focus(); }
      if (what !== "models") redraw("projects");
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
  const place = p && barPlace(p.places, AG.hosts ?? [], pc.machine);
  if (p && !place && !pc.adding && AG.hosts) { c.update({ error: barPrompt(p.name, barReadiness(p.places, AG.hosts, pc.machine, blockedNow())) + "." }); return; }
  if (!p || !place || (!text && !barAttachments.count) || barAttachments.reading || pc.starting || pc.adding) return;
  const t = pc.target;
  if (t?.kind === "issue" && !t.branch.trim()) { c.update({ error: "Name the branch first." }); return; }
  if (t?.kind === "review" && t.earlier) {
    const { earlier } = t;
    return continueInBar(earlier, text, (prompt, images) => steerReview(t.task.number, earlier, prompt, images));
  }
  const { agent } = barHarness(place);
  c.update({ error: "" });
  pc.starting = true;
  pc.open = false;
  redraw("projects");

  try {
    const prompt = await barAttachments.files.prompt(text, place.machine_id, place.cwd);
    if (barAttachments.files.pending.length) c.update({ progress: "Starting the agent…" });
    const images = barAttachments.images.map(({ name, mime, data }) => ({ name, mime, data }));
    const session = await startSession(p.name, place, { ...pc.picked, agent }, prompt, images, t, pc.checkout);
    c.setText("");
    barAttachments.clear();
    pc.target = null;
    go("pane", { agent: session, adopt: true });
  } catch (e) {
    c.update({ error: reason(e) });
  } finally {
    c.update({ progress: "" });
    pc.starting = false;
    redraw("projects");
  }
}

type Image = { name: string; mime: string; data: string };

/// Starts a coding agent of project `name` on `place` with `prompt`: in
/// the current checkout for reviews, a worktree for other targets, or
/// `checkout` without a target. This device answers at once with the
/// session it shows as until its machine names it. A session for a task is
/// named for it then.
async function startSession(name: string, place: BarPlace, pick: Omit<ProjectPick, "agent"> & { agent: string | null }, prompt: string, images: Image[], target: BarTarget | null, checkout: CheckoutPick) {
  const was = barExisting(name, place, target);
  const model = pick.model.trim() || null, effort = pick.effort, fast = pick.agent === "codex" ? pick.fast : null;
  const at = was ? { cwd: was.cwd, checkout: null }
    : target ? { cwd: place.cwd, checkout: targetPlan(target) }
    : startPlace(checkout, place.cwd, baseFor(null, pc.branches.get(name)));
  await loadAccountNames();
  const account = startAccount(accountsOf(pick.agent), name, pick.agent);
  const started = await invoke<Started>("agent_begin", { machineId: place.machine_id, agent: pick.agent, cwd: at.cwd, prompt, model, effort, fast, checkout: at.checkout, images, account });
  if (model || effort || fast !== null) sessionPicks.set(started.session.id, { model: model ?? "", effort, fast });
  startedWith(started.session, prompt, images);
  keepMachine(name, place.machine_id);
  if (target?.task) taskStarts.set(started.session.id, target.task);

  return started.session;
}

/// The task each start is for, by the start's id, until its machine names its session.
const taskStarts = new Map<string, SessionTask>();

listen<AgentStarted>("agent-started", ({ payload: { start, session } }) => {
  const task = taskStarts.get(start);
  if (!task) return;
  taskStarts.delete(start);
  nameSession(session, task);
});

/// What an agent button starts: the prompt and the worktree, or the
/// checkout as it is without one.
export interface AgentTask { prompt: string; target: BarTarget | null }

/// The machine agent buttons of project `name` start on: `machine` when it
/// can, else the one the bar would.
function threadPlace(name: string, machine: string | null) {
  const p = projectOf(name);
  machine ??= pc.project === name ? pc.machine : lastMachine(name);
  return p ? { place: barPlace(p.places, AG.hosts ?? [], machine), why: barPrompt(name, barReadiness(p.places, AG.hosts, machine, blockedNow())) } : null;
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

/// An agent button of project `name` (Work on PR, Review with agent, Work on
/// it, Fix this, Fix with an agent): the picker under it, on `machine` when
/// it can start there, whose last step runs `start` with the machine picked.
/// `choose` offers the project's other machines as its first step.
export function pickButton(name: string, start: (pick: ProjectPick, machine: string | null) => void, machine: string | null = null, choose = true): PickButton {
  const picked: Picked = { agent: "", model: "", effort: null, fast: null, query: "" };
  const listeners = new Set<() => void>();
  let current: ReturnType<PickButton["snapshot"]> = { open: false, picker: null };
  let chosen = machine;
  const host = () => {
    const place = threadPlace(name, chosen)?.place;
    return place ? AG.hosts?.find(h => h.machine_id === place.machine_id) ?? null : null;
  };
  /// The harness to offer first on the machine: the last one picked in the
  /// project when it has it, else the first it has.
  const defaults = () => {
    const offered = allowedOf(host()?.agents ?? [], projectHarnesses(name));
    const saved = lastProjectPick(name);
    if (saved && offered.includes(saved.agent)) Object.assign(picked, saved);
    else if (!offered.includes(picked.agent)) Object.assign(picked, { agent: harnessFor(null, [...offered].sort(byPreference), AGENT_ORDER) ?? "", model: "", effort: null, fast: null });
  };
  const pop = pickPopover({
    picked,
    machine: () => { const h = host(); return h && { id: h.machine_id, name: h.machine_name, agents: [...h.agents].sort(byPreference) }; },
    everywhere: () => [...new Set((AG.hosts ?? []).flatMap(m => m.agents))],
    allowed: () => projectHarnesses(name),
    machines: () => choose ? { rows: projectMachines(name), pick: id => { chosen = id; defaults(); } } : null,
    opening: () => { defaults(); return !!picked.agent; },
    start: () => start({ agent: picked.agent, model: picked.model, effort: picked.effort, fast: picked.fast }, host()?.machine_id ?? chosen),
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
    click: () => void Promise.all([AG.hosts ? null : readProjectHosts(), readProjectHarnesses(name)]).then(() => pop.toggle()),
    why: () => { const at = threadPlace(name, chosen); return !AG.hosts ? "" : at?.place ? "" : `${at?.why ?? `${name} is not on a machine`}.`; },
  };
}

/// The Machine step of project `name`'s pickers: each machine with a coding
/// agent, those without the project's checkout listed but not picked.
function projectMachines(name: string): MachineRow[] {
  const places = projectOf(name)?.places ?? [];
  return machineRows(AG.hosts ?? [], null, agent => agentKind(agent).name, AGENT_ORDER)
    .map(m => places.some(p => p.machine === m.id) ? m : { ...m, enabled: false, note: `${shortName(name)} is not here` });
}

/// The agent buttons of project pages: Fix this, and Fix with an agent in sync.
export const projectButtons = agentButtons(() => { redraw("projects"); redrawSync(); });

/// What one agent button starts, as its owner draws it now.
export interface AgentButtonOpts {
  project: string;
  /// The machine it starts on when it can; null for the bar's.
  machine: string | null;
  /// Whether its picker offers the project's other machines.
  choose: boolean;
  label: string;
  hint: string;
  /// The session it opens, or whether that session is still being looked up.
  session: SessionState | { kind: "checking" } | null;
  start: (pick: ProjectPick, machine: string | null) => Promise<void>;
}

/// Agent buttons kept by key across draws, with the starts going on and why
/// each last failed. A pick runs the `start` of the latest draw.
export function agentButtons(redraw: () => void) {
  const made = new Map<string, PickButton>();
  const latest = new Map<string, AgentButtonOpts>();
  const starting = new Set<string>();
  const failed = new Map<string, string>();

  async function run(key: string, pick: ProjectPick, machine: string | null) {
    const o = latest.get(key);
    if (!o || starting.has(key)) return;
    starting.add(key);
    failed.delete(key);
    redraw();

    try {
      await o.start(pick, machine);
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
    if (!button) made.set(key, button = pickButton(o.project, (pick, machine) => void run(key, pick, machine), o.machine, o.choose));
    const state: AgentState = o.session ? o.session
      : starting.has(key) ? { kind: "starting" } : { kind: "ready", label: o.label, hint: o.hint };

    return { button, state, error: failed.get(key) ?? "" };
  };
}


/// The session that last reviewed PR `number` of `project`, found by its
/// name among the project's sessions on every machine, as it is now.
export const earlierReview = (project: string, number: number) => sessionFor(project, { kind: "review", number }, null);

/// The session of `project` started for `task`, as it is now: the newest
/// named for it on any machine, else the newest chat in `worktree` when the
/// task made its own (as a session started before tasks named theirs),
/// which is named for it then.
function sessionFor(project: string, task: SessionTask, worktree: BarTarget | null): TaskSession | null {
  const found = taskSession(projectSessions(project), task);
  if (found) return sameSession(found) ?? found;
  const earlier = worktree?.kind === "issue" ? branchSession(project, worktree) : null;
  if (earlier) nameSession(earlier, task);
  return earlier;
}

/// The agent button of a task whose session exists: while it works it shows
/// `working` and opens the chat; after, Open session.
export type SessionState = Extract<AgentState, { kind: "running" | "done" }>;

export function sessionState(s: TaskSession | null, working: string): SessionState | null {
  if (!s) return null;
  const open = () => go("pane", { agent: s });
  return AGENT_ACTIVE.has(s.status) ? { kind: "running", label: working, agent: agentLabel(s.agent), hint: "Open the session", open }
    : { kind: "done", label: "Open session", hint: "Opens the agent's chat", open };
}

/// The newest chat of ours in `target`'s worktree on any machine of
/// `project`, as it is now; review chats are left to their PR's reviews.
function branchSession(project: string, target: { branch: string }): TaskSession | null {
  const places = projectOf(project)?.places ?? [];
  const chats = projectSessions(project).filter(s => !reviewTitled(s.title));
  const found = places.map(p => sessionIn(chats, p.machine, targetFolders(target, p.cwd, knownWorktree(project, p.machine, target)))).filter(s => s !== null).sort((a, b) => b.last_activity - a.last_activity)[0];
  return found ? sameSession(found) ?? found : null;
}

/// The sessions of `project` the page read, and those in its checkouts on
/// every machine, which stand in on Home, where no project's are read.
function projectSessions(project: string) {
  const places = projectOf(project)?.places ?? [];
  const inside = (s: TaskSession) => places.some(p => p.machine === s.machine_id && `${s.cwd}/`.startsWith(`${p.cwd.replace(/\/+$/, "")}/`));
  return [...(sessionsOf(project) ?? []), ...sessions.filter(inside)];
}

const sameSession = (s: TaskSession) => sessions.find(l => l.machine_id === s.machine_id && l.agent === s.agent && l.id === s.id) ?? null;

/// Names a session for its task, which finds it again; once per session and
/// app run, so a machine that is offline is not asked on every draw.
function nameSession(s: Pick<TaskSession, "machine_id" | "agent" | "id">, task: SessionTask) {
  const key = `${s.machine_id}\0${s.agent}\0${s.id}`;
  if (named.has(key)) return;
  named.add(key);
  invoke("agent_title_save", { machineId: s.machine_id, agent: s.agent, session: s.id, title: taskTitle(task) })
    .catch(e => console.warn("session title:", e));
}

const named = new Set<string>();

/// Sends the bar's text and attachments to `earlier` through `send`.
async function continueInBar(earlier: TaskSession, text: string, send: (prompt: string, images: Image[]) => Promise<unknown>) {
  const c = projectComp;
  c.update({ error: "" });
  pc.starting = true;
  redraw("projects");

  try {
    const root = sameSession(earlier)?.cwd ?? barProject()?.places.find(p => p.machine === earlier.machine_id)?.cwd ?? "";
    const prompt = await barAttachments.files.prompt(text, earlier.machine_id, root);
    const images = barAttachments.images.map(({ name, mime, data }) => ({ name, mime, data }));
    await send(prompt, images);
    barAttachments.clear();
    c.setText("");
    pc.target = null;
  } catch (e) {
    c.update({ error: reason(e) });
  } finally {
    pc.starting = false;
    redraw("projects");
  }
}

/// Sends `prompt` to the session that reviewed PR `number` before, and
/// names it again when resuming gave it a new id.
async function steerReview(number: number, earlier: TaskSession, prompt: string, images: Image[]) {
  const session = await steer(earlier, prompt, images);
  if (session.id !== earlier.id) nameSession(session, { kind: "review", number });
}

/// Sends `prompt` to `earlier`, resuming it when it ended, and opens it.
async function steer(earlier: TaskSession, prompt: string, images: Image[]) {
  let session = sameSession(earlier);

  if (!session || session.status === "ended") {
    session = (await invoke<Started>("agent_resume", { machineId: earlier.machine_id, agent: earlier.agent, session: earlier.id, prompt: "" })).session;
    reopened(earlier);
    reopened(session);
  }

  try {
    await invoke("agent_steer", { machineId: session.machine_id, agent: session.agent, session: session.id, steer: { prompt: { text: prompt, images } } });
  } catch (e) {
    const kind = typeof e === "object" && e !== null && "kind" in e ? (e as { kind: unknown }).kind : null;
    if (kind !== "waiting") throw e;
  }

  go("pane", { agent: session });
  return session;
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
  redraw("projects");

  try {
    const folder = projectUnder(await projectsIn(m.machine_id), project);
    await invoke<string>("project_add_to_machine", { project, machineId: m.machine_id, folder });
    await new Promise<void>(resolve => {
      let done = false;
      const finish = () => { done = true; unfollow(); if (finishCopy === finish) finishCopy = undefined; resolve(); };
      const take = (detail: SyncDetail | null) => {
        if (done) return;
        if (!here()) { finish(); return; }
        const line = copyLine(detail, m.machine_id, m.machine_name);
        if (line.done) { finish(); return; }
        say([[line.text, line.file].filter(Boolean).join(" · "), line.note].filter(Boolean).join(". "));
      };
      // Read once; the sync and projects updates carry it after that.
      const unfollow = subscribeEach([topic("sync"), topic("projects")], event => {
        const pushed = pushedView<SyncDetail | null>(event.data, "project_sync_detail", { project });
        if (pushed && "value" in pushed) take(pushed.value);
      });
      finishCopy = finish;
      void invoke<SyncDetail | null>("project_sync_detail", { project }).catch(() => null).then(take);
    });
    if (here()) say("");
  } catch (e) {
    if (here()) say(reason(e));
  } finally {
    if (pc.adding === m) pc.adding = null;
    redraw("projects");
  }
}
