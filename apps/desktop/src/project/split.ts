// A project page's split: the list of tasks, PRs, issues and machines on
// the left, the picked row's view on the right.
import { harnessEl } from "../ui/harnessicon.ts";
import { ago, plural } from "../ui/format.ts";
import { dbtn as button, emptyPanel, errorNote, stateChip } from "../ui/parts.ts";
import { pastSessions, type Project, type WhereRow } from "./projects-state.ts";
import { el as h, type Kid } from "../ui/dom.ts";
import { agentFor, pageAgent, tasksSection, workersSection, cardsSection } from "./agent.ts";
import { terminalTile, type TerminalRow } from "../terminals/terminals.ts";
import { branchTile } from "../review/review.ts";
import { taskTile, machineTile, agentTile, askingSessions } from "./board.ts";
import { fileSyncTile } from "./filesync.ts";
import { issueMeta, prChip, prMeta } from "./threads.ts";
import { arranged, listControls, pickBox, startBar } from "./openlist.ts";
import type { StartEntry } from "./openlist-state.ts";
import { state } from "../center/state.ts";
import { tag, line, section } from "../ui/pieces.ts";
import { homeBox } from "../center/home.ts";
import { terminalLine } from "../terminals/view.ts";
import { go } from "../center/views.ts";
import { sessionMenu } from "../agents/sessionmenu.ts";
import { customAgentsSection, whereSection, agentsSection } from "./projects.ts";
import { pa, terms, fsync, revs, thr, details, runs, projectsEl, redrawProjects, agentActs, repoActs, answerCard } from "./sections.ts";
import { sessionsSection, sessionsDue, loadSessions } from "./loads.ts";
import { IC } from "../ui/icons.ts";
import { byRecent, type RunningAgent, runningKey, runningWhen } from "../agents/running-state.ts";
import { runningLineAt } from "../agents/place.ts";

/// The split shows for an imported project whose agent's view has loaded, and with no
/// subscription for every imported project: it has no agent to wait for.
export function splitOn(p: Project) {
  if (!p.imported) return false;
  return state.subscribed ? !!agentFor(state.agents, p.name) && !!state.pview.get(p.name)?.view : true;
}

/// Picks item `key` of the split's list, or the first one with null, and
/// stops the polling of every other item. With `draw` false the caller
/// draws.
export function openTile(key: string | null, draw = true) {
  pa.tile = key;
  if (key !== "sync") fsync.stop();
  if (key !== "review") revs.stop();
  if (!draw) return;
  redrawProjects();
  projectsEl().querySelector(":scope > .cc-view .pj-det")?.scrollTo({ top: 0 });
}

/// One row of the split's list: it shows its detail at the right, or with
/// `open` goes somewhere else (a running agent opens in the pane).
interface SplitItem {
  key: string;
  lead?: Kid;
  title: string;
  sub: string;
  /// Labels, as chips after `sub`.
  tags?: string[];
  /// When it opened, in milliseconds since the Unix epoch; none when unknown.
  opened?: number | null;
  /// What Start N agents takes of an issue.
  start?: StartEntry;
  right?: Kid;
  open?: () => void;
  /// What right-clicking the row does.
  menu?: (e: MouseEvent) => void;
  detail?: () => Node[];
}

/// A list's label filter: the labels its items carry, the one picked, and
/// what picking one does.
export type LabelFilter = { all: string[]; on?: string; pick: (label: string) => void };
/// A group of the list: its title, rows, and a note in place of rows.
type SplitGroup = { title: string; items: SplitItem[]; note?: string; labels?: LabelFilter; controls?: HTMLElement };

/// The label each list is filtered to, by list key.
const listLabel = new Map<string, string>();

/// `items` filtered to the label picked for list `key`, and the chips that
/// pick one; `redraw` runs on a pick. A label gone from every item clears.
export function labelFilter<T>(key: string, items: T[], tags: (i: T) => string[] | undefined, redraw: () => void) {
  const all = [...new Set(items.flatMap(i => tags(i) || []))].sort();
  const pick = listLabel.get(key), on = pick && all.includes(pick) ? pick : undefined;
  const labels: LabelFilter = { all, on, pick: l => {
    if (on === l) listLabel.delete(key); else listLabel.set(key, l);
    redraw();
  } };
  return { items: on ? items.filter(i => tags(i)?.includes(on)) : items, labels };
}

/// A label chip: a click filters its list to it, a second click clears it.
export function labelChip(label: string, f: LabelFilter, tag: "button" | "i") {
  const c = h(tag, `pj-tag lab${label === f.on ? " on" : ""}`, label);
  if (c instanceof HTMLButtonElement) c.type = "button";
  c.addEventListener("click", e => { e.stopPropagation(); f.pick(label); });
  return c;
}

/// The note of a code host list: why it could not be read, or that it reads.
function hostNote(list: unknown[] | null | undefined, error: string, none: string) {
  if (error) return error;
  if (list === undefined) return "Reading…";
  if (list === null) return "The server needs an update before it lists these.";
  return list.length ? undefined : none;
}

/// One row of a list, lit when its key is `lit`.
function splitRow(i: SplitItem, lit: string | null, f?: LabelFilter, pick?: HTMLElement | null) {
  const row = h("button", "pj-li", h("span", "lead", i.lead), h("span", "t", h("b", "", i.title), h("span", "", i.sub, ...(i.tags || []).map(l => f ? labelChip(l, f, "i") : h("i", "pj-tag", l)))), h("span", "r", i.right));
  row.type = "button";
  row.title = i.title;
  if (i.key === lit) row.setAttribute("aria-current", "true");
  row.addEventListener("click", () => i.open ? i.open() : openTile(i.key));
  if (i.menu) row.addEventListener("contextmenu", i.menu);
  return pick ? h("div", "pj-prow", pick, row) : row;
}

/// The project page: everything going on in one list at the left, the
/// picked row's detail and actions at the right.
export function projectSplit(p: Project, rows: WhereRow[], known: boolean, running: RunningAgent[]): Node[] {
  // No view and no agent chat without a subscription: no card, tasks or plan.
  const view = state.pview.get(p.name)?.view ?? null;
  const agent = pageAgent(state.agents, p.name, state.subscribed)!;
  const pid = agent.project_id;
  const sessions = state.sessions.get(p.name);
  if (sessions === undefined || sessionsDue.has(p.name)) loadSessions(p.name);
  if (terms.list(p.name) === undefined) terms.load(p.name);
  revs.ensure(p.name);
  thr.ensure(pid);
  const acts = repoActs(p);
  const sync = state.sync.filter(s => s.project === p.name);
  const cards = view && cardsSection(view, { ...agentActs, answer: (c, decision, said) => answerCard(agent, c, decision, said) });
  const groups: SplitGroup[] = [];
  if (cards) groups.push({ title: "Needs you", items: [{ key: "needs", title: "The project agent waits on you", sub: plural(view.cards.length, "card", "cards"), detail: () => [cards] }] });
  const project: SplitItem[] = [
    { key: "actions", title: "Actions", sub: runs.tile(pid).count || "", detail: () => [runs.section(agent)] },
    // A machine's terminals sit below it, as on Home.
    { key: "machines", title: "Machines", sub: [machineTile(rows, sync).count, terminalTile(terms.list(p.name), ago).count].filter(Boolean).join(" · "), detail: () => [whereSection(p, rows, known), terminalsSection(p.name)] },
    { key: "sync", title: "File sync", sub: fileSyncTile(fsync.forTile(p.name), sync, new Map(rows.map(w => [w.id, w.name]))).count || "", detail: () => fsync.panel(p.name) },
  ];
  const threads = thr.threads(pid);
  if (threads?.length) {
    const waiting = threads.filter(t => t.needs_reply).length;
    project.push({ key: "threads", title: "Your pull requests and issues", sub: waiting ? `${waiting} ${waiting === 1 ? "needs" : "need"} a reply` : `${threads.length} open`, detail: () => thr.threadsPanel(agent) });
  }
  const runItems: SplitItem[] = byRecent(running).map(r => ({
    key: `run:${runningKey(r)}`, lead: harnessEl(r.agent), ...runningLineAt(r, state.home?.projects || []),
    right: r.status === "asking" ? tag("wait", "needs you") : runningWhen(r), open: () => go("pane", { agent: r }),
    menu: e => void sessionMenu(e, r, s => go("pane", { agent: s })),
  }));
  if (runItems.length) groups.push({ title: "Running", items: runItems });
  groups.push({ title: "Project", items: project });
  const past = sessions ? pastSessions(sessions, running) : [];
  groups.push({ title: "Work", items: [
    ...(view ? [{ key: "tasks", title: "Tasks", sub: taskTile(view.tasks, view.workers, askingSessions(running, state.home?.needs_you)).count || "None open", detail: () => [h("div", "pj-cols", tasksSection(view, agentActs), workersSection(view, agentActs, IC.chat))] }] : []),
    { key: "agents", title: "Agents", sub: agentTile(running, past).count || "None running", detail: () => [agentsSection(running), customAgentsSection(rows), sessionsSection(p, running)] },
    { key: "branches", title: "Branches", sub: branchTile(revs.branches(p.name), revs.reviews(p.name), ago).count || "", detail: () => revs.branchesPanel(p.name) },
  ] });
  const num = (n: number) => h("span", "num", `#${n}`);
  const prs = thr.prs(pid), issues = thr.issues(pid);
  const prItems: SplitItem[] = thr.prLines(agent, acts).map(l => {
    const chip = prChip(l), pr = l.pr;
    return {
      key: `pr:${pr.number}`, opened: pr.created_at, lead: num(pr.number), title: pr.title, sub: prMeta(l, ago).join(" · "), tags: pr.labels,
      right: chip && stateChip(chip.cls, chip.text, chip.cls === "running"),
      menu: e => thr.prMenu(e, agent, acts, l),
      detail: () => details.pane(pid, { ...pr, author: pr.mine ? "you" : pr.author, opened_at: pr.created_at }, [thr.prButtons(agent, acts, l)]),
    };
  });
  const prKey = `${p.name}:prs`;
  const prFilter = labelFilter(prKey, prItems, i => i.tags, redrawProjects);
  groups.push({ title: "Pull requests", note: hostNote(prs, thr.prsError(pid), "No open pull requests."), controls: listControls(prKey, redrawProjects, false), labels: prFilter.labels, items: arranged(prKey, prFilter.items, i => i.opened) });
  const issueItems: SplitItem[] = thr.issueLines(agent, acts).map(l => ({
    key: `issue:${l.number}`, opened: l.issue?.opened_at, start: l.issue ? { key: `issue:${l.number}`, project: p.name, number: l.number, title: l.title, url: l.url } : undefined,
    lead: num(l.number), title: l.title,
    sub: l.issue ? issueMeta(l.issue, ago) : `task: ${l.task!.title}`, tags: l.issue?.labels,
    right: l.task && stateChip(l.task.state, l.task.state, l.task.state === "running"),
    menu: l.issue ? (e => thr.issueMenu(e, agent, acts, l.issue!)) : undefined,
    detail: () => details.pane(pid, { number: l.number, title: l.title, url: l.url, author: l.issue?.author || "", opened_at: l.issue?.opened_at || 0 },
      [l.issue ? thr.issueButtons(agent, acts, l.issue) : button("Tasks", "", () => openTile("tasks"))]),
  }));
  const issueKey = `${p.name}:issues`;
  const issueFilter = labelFilter(issueKey, issueItems, i => i.tags, redrawProjects);
  const issueShown = arranged(issueKey, issueFilter.items, i => i.opened);
  const issueNote = hostNote(issues, thr.issuesError(pid), "No open issues.");
  // Too many to list at the left: the row opens them at the right.
  project.push({ key: "issues", title: "Issues", sub: issueNote || `${issueItems.length} open`, detail: () => [
    listControls(issueKey, redrawProjects, true),
    h("div", "pj-labels", ...issueFilter.labels.all.map(l => labelChip(l, issueFilter.labels, "button"))),
    h("div", "pj-rows", ...issueShown.map(i => splitRow(i, null, issueFilter.labels, i.start ? pickBox(issueKey, i.key, redrawProjects) : null))),
    startBar(issueKey, issueShown.flatMap(i => i.start ?? []), redrawProjects),
  ] });
  const overview = projectOverview(p, running, { run: runItems, prs: prItems, issues: issueItems, prNote: hostNote(prs, thr.prsError(pid), "No open pull requests."), issueNote }, machineTile(rows, sync));
  groups.unshift({ title: "", items: [{ key: "overview", title: "Overview", sub: overview.sub, detail: () => [overview.body] }] });
  // A review opens from Branches and an issue from Issues, each keeping its row lit.
  const all = groups.flatMap(g => g.items);
  const issue = issueItems.find(i => i.key === pa.tile);
  const picked = pa.tile === "review"
    ? { key: "branches", detail: () => revs.reviewPanel(p.name) }
    : issue ? { key: "issues", detail: () => [button("All issues", "quiet small pj-back", () => openTile("issues")), ...issue.detail!()] }
    : all.find(i => i.key === pa.tile && i.detail) || all[0];
  const list = h("div", "pj-list");
  for (const g of groups) {
    if (g.title) list.append(h("h4", "", g.title, g.items.length ? h("span", "n", String(g.items.length)) : null));
    if (g.note) list.append(h("div", "note", g.note));
    if (g.controls) list.append(g.controls);
    const f = g.labels;
    if (f?.all.length) list.append(h("div", "pj-labels", ...f.all.map(l => labelChip(l, f, "button"))));
    for (const i of g.items) {
      list.append(splitRow(i, picked.key, f));
    }
  }
  const error = state.pview.get(p.name)?.error;
  const det = h("div", "pj-det", error ? errorNote(`Could not refresh the project: ${error}`) : null, ...picked.detail!());
  return [h("div", "pj-split", list, det)];
}

/// The agent view with terminal `t`, one of project `name`, in a pane tab with the keys.
function openTerminal(name: string, t: TerminalRow) {
  go("pane", { draft: { project: name, machine: t.machine_id, terminal: t } });
}

/// The project's terminals, which keep running on their machines; a row opens one.
function terminalsSection(name: string) {
  const list = terms.list(name) ?? [];
  const body = list.length ? h("div", "cc-panel", ...list.map(t => terminalLine(t, () => openTerminal(name, t)))) : emptyPanel(h("span", "", "None yet. ⌘T opens one."));
  return section("", "Terminals", list.length || "", null, body);
}

/// The project's Overview: what runs and ran on the left, the open work on
/// the right, each title opening that row of the list. Issues scroll in their box.
function projectOverview(p: Project, running: RunningAgent[], open: { run: SplitItem[]; prs: SplitItem[]; issues: SplitItem[]; prNote: Kid; issueNote: Kid }, where: ReturnType<typeof machineTile>) {
  const { run, prs, issues } = open;
  // A copy: the list beside it takes the lead node itself.
  const rowsOf = (items: SplitItem[]) => items.map(i => {
    const row = line(i.lead instanceof Node ? i.lead.cloneNode(true) : null, i.title, i.sub, [i.right, ...(i.tags || []).slice(0, 3).map(l => h("i", "pj-tag", l))], i.open || (() => openTile(i.key)));
    if (i.menu) row.addEventListener("contextmenu", i.menu);
    return row;
  });
  const box = (title: string, items: SplitItem[], key: string) => items.length ? homeBox("", title, items.length, () => openTile(key), rowsOf(items)) : null;
  const tile = runs.tile(pageAgent(state.agents, p.name, state.subscribed)?.project_id || "");
  const ran = tile.rows.map(r => line(null, r.text || "", r.code || "", [r.chip ? stateChip(r.chip.cls, r.chip.text, r.chip.cls === "running") : null], () => openTile("actions")));
  const list = terms.list(p.name) || [];
  const machines = where.rows.map(r => {
    const row = line(h("span", `ddot${r.dot ? " off" : ""}`), r.text || "", r.note || "", [], () => openTile("machines"));
    row.title = r.title || r.text || "";
    return row;
  });
  const left = [
    box("Running", run, "agents"),
    sessionsSection(p, running),
    ran.length ? homeBox("", "Actions", tile.count, () => openTile("actions"), ran) : null,
    machines.length ? homeBox("", "Machines", where.count, () => openTile("machines"), machines) : null,
    list.length ? homeBox("", "Terminals", list.length, () => openTile("machines"), list.map(t => terminalLine(t, () => openTerminal(p.name, t)))) : null,
  ];
  const right = [
    prs.length ? box("Pull requests", prs, prs[0].key) : section("", "Pull requests", "", null, emptyPanel(h("span", "", open.prNote))),
    issues.length
      ? section("", button("Issues", "lk", () => openTile("issues")), issues.length, null, h("div", "cc-panel pj-scroll", ...rowsOf(issues)))
      : section("", "Issues", "", null, emptyPanel(h("span", "", open.issueNote))),
  ];
  const sub = [run.length ? `${run.length} running` : "", prs.length ? plural(prs.length, "PR", "PRs") : "", issues.length ? plural(issues.length, "issue", "issues") : ""].filter(Boolean).join(" · ");
  return { sub, body: h("div", "pj-ov", h("div", "", ...left), h("div", "", ...right)) };
}
