// The bar under a project page: its prompt, harness picker and checkout, and
// starting or resuming a coding agent from it.
import { reason } from "../ui/format.ts";
import { invoke } from "../ui/tauri.ts";
import { filterProjects } from "./projects-state.ts";
import { el as h } from "../ui/dom.ts";
import { openMenu } from "../ui/menu.ts";
import { pageAgent } from "./agent.ts";
import { type BarTarget, barCheckout, barPlace, barPrompt, forgeOf, reviewPrompt, sessionIn, targetCheckout, targetFolders, targetPlan } from "./projectbar.ts";
import { keepMachine, lastMachine } from "../agents/agentnew.ts";
import { type BaseBranches, baseFor, type CheckoutPick, DEFAULT_CHECKOUT, harnessFor, keepEffort, knownWorktrees, startPlace } from "../agents/agentpick.ts";
import { closedPicker, drawPicker, focusSearch, modelsOn, pillInner, preloadModels } from "../agents/agentpicker.ts";
import { AG, AGENT_ORDER, type AgentHost, byPreference } from "../agents/agents.ts";
import { draftPlace } from "../agents/draft.ts";
import { S } from "../bar/core.ts";
import { loadSessions, sessions } from "../agents/sessions.ts";
import { issueBranch, issuePrompt, type IssueRow, type PullRequestHead, type PullRequestRow } from "./threads.ts";
import { planStarts, type StartEntry, type StartRow } from "./openlist-state.ts";
import { fixPrompt, runTarget, type RunDetail } from "./runs.ts";
import { conflictPrompt, type Conflict } from "./filesync.ts";
import { type Started, state } from "../center/state.ts";
import { shown, go } from "../center/views.ts";
import { projectRows } from "../center/sidebar.ts";
import { redrawProjects, pc } from "./sections.ts";
import { projectPlaces } from "./loads.ts";

/// The tallest the bar's field grows before it scrolls, in pixels.
const BAR_FIELD_MAX = 200;

export const projectComp = (() => {
  // A field that wraps and grows with what is typed, like a chat's:
  // Enter starts, Shift+Enter adds a line.
  const input = h("textarea");
  input.rows = 1;
  input.autocomplete = "off";
  input.spellcheck = false;
  input.setAttribute("aria-label", "Task for a coding agent");
  input.title = "Enter starts, Shift+Enter adds a line";
  const machine = h("button", "pill");
  machine.type = "button";
  machine.title = "The machine it runs on";
  const folder = h("button", "pill");
  folder.type = "button";
  folder.title = "The checkout, a new worktree, or an existing worktree";
  const harness = h("button", "pill");
  harness.type = "button";
  harness.title = "Harness, model and thinking";
  const pick = h("div", "pick");
  const send = h("button", "dbtn pri", "Start", h("kbd", "", "⏎"));
  send.type = "submit";
  // Work on it's branch, editable, and Clear for what Work on it or
  // Continue put in the bar.
  const branch = h("input", "branch");
  branch.type = "text";
  branch.spellcheck = false;
  branch.autocomplete = "off";
  branch.setAttribute("aria-label", "Branch name");
  branch.title = "The new branch, in a new worktree";
  branch.addEventListener("input", () => { if (pc.target?.kind === "issue") pc.target.branch = branch.value; });
  const clear = h("button", "dbtn quiet small", "Clear");
  clear.type = "button";
  clear.title = "Back to a plain start";
  clear.addEventListener("click", () => { pc.target = null; input.value = ""; redrawProjects(); });
  const err = h("span", "cc-err");
  err.setAttribute("role", "status");
  const row = h("div", "row", machine, folder, branch, clear, h("span", "sp"), harness, send);
  const form = h("form", "cc-pjc", h("div", "box", input, row, pick), err);
  form.addEventListener("submit", e => { e.preventDefault(); startFromBar(); });
  input.addEventListener("keydown", e => {
    if (e.key !== "Enter" || e.shiftKey || e.isComposing) return;
    e.preventDefault();
    form.requestSubmit();
  });
  input.addEventListener("input", () => growBarField(input));
  machine.addEventListener("click", e => {
    const p = barProject();
    if (!p) return;
    const able = (pc.hosts ?? []).filter(m => m.agents.length && p.places.some(x => x.machine === m.machine_id));
    openMenu(e, "Machine", [
      { label: "Auto", on: () => { pc.machine = "auto"; redrawProjects(); } },
      ...able.map(m => ({ label: m.machine_name, on: () => { pc.machine = m.machine_id; keepMachine(p.name, m.machine_id); redrawProjects(); } })),
    ]);
  });
  folder.addEventListener("click", e => {
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
  });
  // The start form's picker, here above the bar.
  harness.addEventListener("click", () => {
    pc.open = !pc.open;
    pc.picked.query = "";
    redrawProjects();
    if (pc.open) focusSearch(pick);
  });
  // Esc or a click outside closes it.
  form.addEventListener("keydown", e => {
    if (e.key !== "Escape" || !pc.open) return;
    e.preventDefault();
    e.stopPropagation();
    closeBarPicker();
    harness.focus();
  });
  addEventListener("mousedown", e => {
    if (pc.open && !pick.contains(e.target as Node) && !harness.contains(e.target as Node)) closeBarPicker();
  }, true);
  return { form, input, machine, folder, branch, clear, harness, send, err, pick };
})();

function closeBarPicker() {
  pc.open = false;
  redrawProjects();
}

/// The machine the bar starts on, its harnesses, and the harness picked
/// there, kept with the model when the machine or harness changes.
function barHarness(place: { machine_id: string } | null) {
  const host = place ? pc.hosts?.find(m => m.machine_id === place.machine_id) ?? null : null;
  const offered = host ? [...host.agents].sort(byPreference) : [];
  const agent = harnessFor(pc.picked.agent || null, offered, AGENT_ORDER);
  if (agent && agent !== pc.picked.agent) pc.picked.effort = keepEffort(agent, pc.picked.effort);
  pc.picked.agent = agent ?? "";
  const key = host && agent ? `${host.machine_id}/${agent}` : "";
  if (pc.modelFor !== key) { pc.picked.model = ""; pc.modelFor = key; }
  return { host, offered, agent };
}

/// The bar's project and its checkouts.
function barProject() {
  const p = pc.project ? filterProjects(projectRows() || [], pc.project)[0] : null;
  return p ? { name: p.name, places: projectPlaces(p) } : null;
}

/// Whether the bar of project `name` is on screen: under its page, or in the
/// empty agent view.
export function barOn(name: string) {
  return (shown === "projects" && state.picked === name) || (shown === "pane" && S.view === "agent" && AG.draft === name);
}

/// Reads what the bar needs for project `name`: the machines that run
/// agents, the sessions (for worktrees) and the project's branches.
function loadProjectBar(name: string) {
  const redraw = () => { if (barOn(name)) redrawProjects(); };
  invoke<AgentHost[]>("agent_hosts").then(list => { pc.hosts = list; preloadModels(list); redraw(); }).catch(e => { console.warn("agent hosts:", e); pc.hosts ??= []; redraw(); });
  loadSessions().then(redraw).catch(e => console.warn("agent sessions:", e));
  if (!pc.branches.has(name)) {
    invoke<BaseBranches | null>("project_branches", { project: name })
      .catch(() => null)
      .then(list => { pc.branches.set(name, list); redraw(); });
  }
}

/// Fits the bar's field to its text, up to BAR_FIELD_MAX, then it scrolls.
/// Only once it is on screen: before that it measures nothing.
function growBarField(input: HTMLTextAreaElement) {
  if (!input.isConnected) return;
  input.style.height = "auto";
  input.style.height = `${Math.min(input.scrollHeight, BAR_FIELD_MAX)}px`;
  input.style.overflowY = input.scrollHeight > BAR_FIELD_MAX ? "auto" : "hidden";
}

/// Points the bar at project `name`: its field, machine, folder and harness.
export function drawProjectComp(name: string) {
  const c = projectComp;
  if (pc.project !== name) {
    pc.project = name;
    pc.machine = lastMachine(name) || "auto";
    pc.checkout = DEFAULT_CHECKOUT;
    pc.target = null;
    c.err.textContent = "";
    loadProjectBar(name);
  }
  const p = barProject();
  // The machine last picked, while it still has a checkout and an agent.
  let place = p && barPlace(p.places, pc.hosts ?? [], pc.machine);
  if (p && !place && pc.machine !== "auto" && pc.hosts) place = barPlace(p.places, pc.hosts, "auto");
  const host = place ? pc.hosts?.find(m => m.machine_id === place.machine_id) ?? null : null;
  c.input.placeholder = barPrompt(name, !!place || pc.hosts === null);
  // Text set or cleared from code (Work on it, a start) fits once drawn.
  requestAnimationFrame(() => growBarField(c.input));
  c.machine.replaceChildren(place ? place.machine_name : pc.machine === "auto" ? "Auto" : "No checkout", ...(place?.auto ? [" ", h("small", "", "Auto")] : []));
  const t = pc.target;
  const co = t ? targetCheckout(t) : barCheckout(pc.checkout, baseFor(null, pc.branches.get(name)));
  c.folder.replaceChildren(co.label, " ", h("small", "", co.sub));
  c.folder.disabled = !place;
  c.branch.hidden = t?.kind !== "issue";
  if (t?.kind === "issue" && document.activeElement !== c.branch) c.branch.value = t.branch;
  c.clear.hidden = !t;
  const { offered, agent } = barHarness(place);
  if (agent && host) {
    modelsOn(host.machine_id, agent, () => { if (barOn(name)) redrawProjects(); });
    c.harness.innerHTML = pillInner(agent, pc.picked.model, pc.picked.effort, host.machine_id);
  } else {
    c.harness.replaceChildren("Coding agent");
  }
  c.harness.disabled = !agent;
  if (!agent) pc.open = false;
  c.harness.setAttribute("aria-expanded", String(pc.open));
  drawBarPicker(host, offered);
  c.send.disabled = pc.starting || !place;
  c.send.firstChild!.textContent = pc.starting ? "Starting" : "Start";
  draftPlace(place);
}

/// The newest session of ours in the bar target's worktree on the bar's
/// machine, or null; none without a target.
function barExisting(name: string, place: { machine_id: string; cwd: string }) {
  const t = pc.target;
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
  projectComp.input.value = text;
  projectComp.err.textContent = "";
  // Read again: a session may have started in the worktree since.
  loadSessions().then(redrawProjects).catch(e => console.warn("agent sessions:", e));
  redrawProjects();
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
  const base = baseFor(null, pc.branches.get(name));
  fillBar(name, issuePrompt(issue), { kind: "issue", number: issue.number, branch: issueBranch(issue.number, issue.title), base });
}

/// Where each picked issue starts, on the machine and harness the bar has
/// picked (Auto and the first harness for another project); null until the
/// machines are read.
export function issueStartPlan(entries: readonly StartEntry[]) {
  if (!pc.hosts) return null;
  const projects = projectRows() || [];
  const places = (name: string) => { const p = projects.find(x => x.name === name); return p ? projectPlaces(p) : []; };
  return planStarts(entries, places, pc.hosts, name => (pc.project === name ? pc.machine : "auto"), pc.picked.agent || null, AGENT_ORDER);
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
      await invoke<Started>("agent_start", { machineId: machine_id, agent, cwd, prompt: issuePrompt(entry), chat: null, model, effort: same ? pc.picked.effort : null, checkout });
    } catch (e) {
      return { started, failed: row, error: reason(e) };
    }
    started.push(row);
    done(started);
  }
  return { started, failed: null, error: null };
}

/// Continue: reads the pull request's branch, then fills the bar to start
/// on it. A fork's branch is not on `origin`, so it is refused.
export async function continuePr(name: string, pr: PullRequestRow) {
  const agent = pageAgent(state.agents, name, state.subscribed);
  if (!agent) return;
  const head = await invoke<PullRequestHead>("center_pull_request_head", { projectId: agent.project_id, number: pr.number });
  if (head.fork) throw new Error(`PR #${pr.number} comes from a fork, so its branch ${head.branch} is not in this repository.`);
  fillBar(name, `Continue PR #${pr.number}: ${pr.title} (${pr.url}). Read the review comments and failing checks first.`, { kind: "pr", number: pr.number, branch: head.branch });
}

/// Agent review: fills the bar with a review prompt. A fork's branch is not
/// on `origin`, so that review starts on a new worktree from the default
/// branch and reads the diff through the forge CLI.
export async function reviewPr(name: string, pr: PullRequestRow) {
  const agent = pageAgent(state.agents, name, state.subscribed);
  if (!agent) return;
  const head = await invoke<PullRequestHead>("center_pull_request_head", { projectId: agent.project_id, number: pr.number });
  const forge = forgeOf(pr.url);
  const target: BarTarget = head.fork
    ? { kind: "issue", number: pr.number, branch: `review-pr-${pr.number}`, base: baseFor(null, pc.branches.get(name)) }
    : { kind: "pr", number: pr.number, branch: head.branch };
  fillBar(name, reviewPrompt(pr, forge, head.fork), target);
}

/// Fix this: the bar on the failed run's branch (a new branch when it ran
/// on the default branch), with the failure and its log in the prompt.
export function fixRun(name: string, detail: RunDetail) {
  fillBar(name, fixPrompt(detail), runTarget(detail.run, baseFor(null, pc.branches.get(name))));
}

/// Fix with an agent: the project's bar filled to settle a sync conflict, on
/// a machine that has it. The user sends it.
export function fixConflict(name: string, c: Conflict) {
  if (!c.holder) return;
  go("projects", { project: name });
  fillBar(name, conflictPrompt(c), null);
  pc.machine = c.holder;
  redrawProjects();
}

/// The picker above the bar, when open.
function drawBarPicker(host: AgentHost | null, offered: string[]) {
  const el = projectComp.pick, p = pc.picked;
  if (!pc.open || !host || !p.agent) { el.innerHTML = ""; closedPicker(el); return; }
  const focused = el.contains(document.activeElement);
  drawPicker(el, {
    picked: p,
    offered,
    everywhere: [...new Set((pc.hosts ?? []).flatMap(m => m.agents))],
    machine: { id: host.machine_id, name: host.machine_name },
    noMachine: "",
    anchor: () => projectComp.harness,
    bounds: () => projectComp.form.parentElement,
    changed: what => {
      if (what === "model") { pc.open = false; redrawProjects(); return projectComp.input.focus(); }
      if (what === "agent") p.effort = keepEffort(p.agent, p.effort);
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
  const c = projectComp, p = barProject(), text = c.input.value.trim();
  let place = p && barPlace(p.places, pc.hosts ?? [], pc.machine);
  if (p && !place && pc.machine !== "auto") place = barPlace(p.places, pc.hosts ?? [], "auto");
  if (!p || !place || !text || pc.starting) return;
  const t = pc.target;
  if (t?.kind === "issue" && !t.branch.trim()) { c.err.textContent = "Name the branch first."; return; }
  const was = barExisting(p.name, place);
  const { agent } = barHarness(place);
  const model = pc.picked.model.trim() || null, effort = pc.picked.effort;
  // A session already in the target's worktree: the new one goes beside it.
  const { cwd, checkout } = was ? { cwd: was.cwd, checkout: null }
    : t ? { cwd: place.cwd, checkout: targetPlan(t) }
    : startPlace(pc.checkout, place.cwd, baseFor(null, pc.branches.get(p.name)));
  c.err.textContent = "";
  pc.starting = true;
  pc.open = false;
  redrawProjects();
  try {
    const started = await invoke<Started>("agent_start", { machineId: place.machine_id, agent, cwd, prompt: text, chat: null, model, effort, checkout });
    c.input.value = "";
    pc.target = null;
    keepMachine(p.name, place.machine_id);
    go("pane", { agent: started.session, adopt: true });
  } catch (e) {
    c.err.textContent = reason(e);
  } finally {
    pc.starting = false;
    redrawProjects();
  }
}
