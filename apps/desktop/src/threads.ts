// A project's issues and the user's pull requests and issues there. The
// board's Issues tile and the Pull requests tile's rows, and their full
// lists: an open issue no task tracks gets Work on it (agent and machine),
// Plan, Discuss and Not now; a thread of the user's gets Let the agent
// reply when someone wrote after them, and Discuss. center.ts draws the
// page; this file keeps the state.
import type { invoke as Invoke } from "./tauri.ts";
import { agentLabel, reason } from "./projects-state.ts";
import type { Ago, MakeButton, MakeEl } from "./projects-state.ts";
import type { ProjectAgent } from "./project.ts";
import type { TileData } from "./board.ts";
import { plural } from "./components/format.ts";
import { emptyPanel, segmented, stateChip } from "./components/parts.ts";

/// An open issue no task tracks (IssueRow).
export interface IssueRow {
  number: number;
  title: string;
  labels: string[];
  author: string;
  /// In milliseconds since the Unix epoch.
  opened_at: number;
  comments: number;
  url: string;
}

/// One of the user's open pull requests or issues in a project (ThreadRow).
export interface ThreadRow {
  /// `pr` or `issue`.
  kind: string;
  repo: string;
  number: number;
  title: string;
  url: string;
  /// Who wrote last after the user, and what.
  last_other: string;
  needs_reply: boolean;
  /// In milliseconds since the Unix epoch.
  updated_at: number;
}

/// An open pull request of the project's, anyone's (PullRequestRow).
export interface PullRequestRow {
  number: number;
  title: string;
  url: string;
  author: string;
  draft: boolean;
  /// Opened by the account the host's CLI is signed in as.
  mine: boolean;
  /// In milliseconds since the Unix epoch.
  created_at: number;
  /// In milliseconds since the Unix epoch.
  updated_at: number;
}

/// A chat about one thread (ThreadChatRow).
export interface ThreadChatRow {
  chat_id: string;
  /// `pr` or `issue`.
  kind: string;
  repo: string;
  number: number;
  title: string;
}

/// A machine and the coding agents it offers (HostRow).
export interface HostRow {
  machine_id: string;
  machine_name: string;
  agents: string[];
  workspace: string | null;
  home: string | null;
  capabilities: string[];
}

/// What Work on it offers.
export interface WorkChoices {
  agents: string[];
  machines: { id: string; name: string; agents: string[] }[];
  agent: string | null;
  machine: string | null;
}

/// A machine row as `workChoices` reads it: `whereRows` gives them.
type Where = { id: string; kind: string };

/// "#52", "PR #41".
export function threadCode(t: Pick<ThreadRow, "kind" | "number">) {
  return t.kind === "pr" ? `PR #${t.number}` : `#${t.number}`;
}

/// The Issues tile from the open issues no task tracks; undefined while
/// they load, null from a server too old to list them.
export function issueTile(list: readonly Pick<IssueRow, "number" | "title" | "url" | "labels" | "comments">[] | null | undefined): TileData {
  if (list === undefined) return { count: "", rows: [], empty: "Loading…" };
  if (list === null) return { count: "", rows: [], empty: "The server lists no issues yet" };
  return {
    count: list.length ? `${list.length} untracked` : "",
    rows: list.map(i => ({ code: `#${i.number}`, text: i.title, url: i.url, note: i.labels?.[0] || (i.comments ? `${i.comments} comments` : "") })),
    empty: "No untracked issues",
  };
}

/// An open PR as `threadTile` reads it.
type OpenPr = Pick<PullRequestRow, "number" | "title" | "url" | "author" | "draft" | "mine" | "created_at">;

/// The Pull requests tile. With the project's open PRs (`open`): the threads
/// waiting on the user's reply first, then every other open PR newest
/// first, someone else's noting its author. Without them (loading, or a
/// server too old to list them), the user's threads: their open PRs, those
/// waiting on a reply first; `fallback` (the tasks' PRs) while those load
/// or on a server too old to list them.
export function threadTile(
  list: readonly (Pick<ThreadRow, "kind" | "number" | "title" | "needs_reply"> & Partial<Pick<ThreadRow, "url">>)[] | null | undefined,
  fallback: TileData,
  open?: readonly OpenPr[] | null,
): TileData {
  if (Array.isArray(open)) {
    const threads = Array.isArray(list) ? list : [];
    const waiting = threads.filter(t => t.needs_reply);
    const waitingPrs = new Set(waiting.filter(t => t.kind === "pr").map(t => t.number));
    const mine = new Set(threads.filter(t => t.kind === "pr").map(t => t.number));
    const byNumber = new Map(open.map(p => [p.number, p]));
    const rest = open.filter(p => !waitingPrs.has(p.number)).sort((a, b) => b.created_at - a.created_at || b.number - a.number);
    /// The author of someone else's PR; a draft of the user's says so.
    const note = (p: OpenPr | undefined) => !p ? "" : p.mine || mine.has(p.number) ? (p.draft ? "draft" : "") : p.author;
    const count = rest.length + waitingPrs.size;
    return {
      count: count ? `${count} open` : "",
      rows: [
        ...waiting.map(t => ({
          code: threadCode(t).replace(/^PR /, ""),
          text: t.title,
          url: t.url,
          chip: { cls: "wait", text: "reply" },
          note: t.kind === "pr" ? note(byNumber.get(t.number)) : "",
        })),
        ...rest.map(p => ({ code: `#${p.number}`, text: p.title, url: p.url, chip: null, note: note(p) })),
      ],
      empty: "No open pull requests",
    };
  }
  if (!Array.isArray(list)) return fallback;
  const prs = list.filter(t => t.kind === "pr");
  const waiting = list.filter(t => t.needs_reply).length;
  return {
    count: waiting ? `${waiting} ${waiting === 1 ? "needs" : "need"} reply` : prs.length ? `${prs.length} open` : "",
    rows: [...list.filter(t => t.needs_reply), ...prs.filter(t => !t.needs_reply)].map(t => ({
      code: threadCode(t).replace(/^PR /, ""),
      text: t.title,
      url: t.url,
      chip: t.needs_reply ? { cls: "wait", text: "reply" } : null,
    })),
    empty: fallback?.rows?.length ? fallback.empty : "No open pull requests of yours",
  };
}

/// The line under an issue: who opened it, when, its labels and comments.
export function issueMeta(i: Partial<Pick<IssueRow, "author" | "opened_at" | "labels" | "comments">>, ago: Ago) {
  const out = [`opened by ${i.author || "someone"}`];
  if (i.opened_at) out.push(ago(i.opened_at));
  out.push(...(i.labels || []));
  if (i.comments) out.push(plural(i.comments, "comment", "comments"));
  return out.join(" · ");
}

/// The line under a thread: who wrote last, else that nothing waits.
export function threadMeta(t: Pick<ThreadRow, "last_other" | "updated_at" | "needs_reply">, ago: Ago) {
  const out = [];
  if (t.last_other) out.push(t.last_other);
  if (t.updated_at) out.push(ago(t.updated_at));
  if (!t.needs_reply) out.push("your last comment is the newest");
  return out.join(" · ");
}

/// What Work on it offers: the agents the machines offer, in the order
/// first seen, and the machines with the project, then "Pick at start".
/// The first agent offered is picked.
export function workChoices(hosts: readonly Pick<HostRow, "machine_id" | "machine_name" | "agents">[] | null | undefined, where: readonly Where[] | null | undefined): WorkChoices {
  const agents: string[] = [];
  for (const h of hosts || []) for (const a of h.agents || []) if (!agents.includes(a)) agents.push(a);
  const here = new Set((where || []).filter(r => r.kind === "here").map(r => r.id));
  const machines = (hosts || []).filter(h => here.has(h.machine_id)).map(h => ({ id: h.machine_id, name: h.machine_name, agents: h.agents || [] }));
  return { agents, machines, agent: agents[0] || null, machine: machines[0]?.id || null };
}

/// What the window hands `makeThreads`. `openChat(chatId)` shows a chat in
/// the pane; `changed()` reloads the project's tasks once an issue became one.
export interface ThreadDeps {
  invoke: typeof Invoke;
  h: MakeEl;
  button: MakeButton;
  ago: Ago;
  redraw: () => void;
  openUrl: (url: string) => unknown;
  openChat: (chat: string) => void;
  changed: () => unknown;
}

/// The issue whose Work on it picker shows, and what it picked; undefined
/// until the picker first draws.
interface IssuePick {
  number: number;
  agent?: string | null;
  machine?: string | null;
}

/// One project's lists: undefined while they load, null from a server too
/// old to give them.
interface ProjectThreads {
  issues: IssueRow[] | null | undefined;
  threads: ThreadRow[] | null | undefined;
  prs: PullRequestRow[] | null | undefined;
  chats: ThreadChatRow[] | null | undefined;
  error: string;
  busy: Set<string>;
  pick: IssuePick | null;
}

/// The project agent the lists belong to.
type Agent = Pick<ProjectAgent, "project_id" | "name">;

export function makeThreads(deps: ThreadDeps) {
  const { invoke, h, button, ago, redraw, openUrl, openChat, changed } = deps;
  const all = new Map<string, ProjectThreads>();
  let hosts: HostRow[] | null | undefined;

  const of = (id: string) => {
    let s = all.get(id);
    if (!s) all.set(id, s = { issues: undefined, threads: undefined, prs: undefined, chats: undefined, error: "", busy: new Set(), pick: null });
    return s;
  };

  /// Projects whose first read is on its way; a draw asking again waits.
  const first = new Set<string>();

  async function load(id: string) {
    const s = of(id);
    if (s.issues === undefined && first.has(id)) return;
    first.add(id);
    const [issues, threads, chats, prs] = await Promise.allSettled([
      invoke<IssueRow[] | null>("center_project_issues", { projectId: id }),
      invoke<ThreadRow[] | null>("center_project_threads", { projectId: id }),
      invoke<ThreadChatRow[] | null>("center_thread_chats", { projectId: id }),
      invoke<PullRequestRow[] | null>("center_project_pull_requests", { projectId: id }),
    ]);
    first.delete(id);
    s.error = "";
    /// What a list becomes: what came back, else what it had or none.
    const settle = <T>(r: PromiseSettledResult<T[] | null>, had: T[] | null | undefined) => {
      if (r.status === "fulfilled") return r.value;
      s.error = reason(r.reason);
      return had ?? [];
    };
    s.issues = settle(issues, s.issues);
    s.threads = settle(threads, s.threads);
    s.chats = settle(chats, s.chats);
    // The host's error shows with the issues; without the list the tile
    // shows the user's own threads as before.
    if (prs.status === "fulfilled") s.prs = prs.value;
    else { console.warn("pull requests:", prs.reason); s.prs ??= null; }
    redraw();
  }

  async function loadHosts() {
    if (hosts !== undefined) return;
    hosts = null;
    try { hosts = await invoke<HostRow[]>("agent_hosts"); } catch (e) { console.warn("agent hosts:", e); hosts = []; }
    redraw();
  }

  /// Runs `work` for row `key`, showing it busy and its error.
  async function act(id: string, key: string, work: () => Promise<void>) {
    const s = of(id);
    if (s.busy.has(key)) return;
    s.busy.add(key);
    s.error = "";
    redraw();
    try {
      await work();
    } catch (e) {
      s.error = reason(e);
    } finally {
      s.busy.delete(key);
      redraw();
    }
  }

  const discuss = (agent: Agent, number: number) => act(agent.project_id, `d${number}`, async () => {
    const chat = await invoke<ThreadChatRow>("center_open_thread_chat", { projectId: agent.project_id, repo: agent.name, number });
    const s = of(agent.project_id);
    s.chats = [chat, ...(s.chats || []).filter(c => c.chat_id !== chat.chat_id)];
    openChat(chat.chat_id);
  });

  /// Makes issue `number` a task; `start` starts a worker on it too.
  function track(agent: Agent, number: number, start: { agent: string | null | undefined; machine: string | null | undefined } | null) {
    return act(agent.project_id, `i${number}`, async () => {
      await invoke("center_track_issue", { projectId: agent.project_id, number, agent: start?.agent ?? null, nodeId: start?.machine ?? null });
      const s = of(agent.project_id);
      s.issues = (s.issues || []).filter(i => i.number !== number);
      s.pick = null;
      changed();
    });
  }

  function dismiss(agent: Agent, number: number) {
    return act(agent.project_id, `i${number}`, async () => {
      await invoke("center_dismiss_issue", { projectId: agent.project_id, number });
      const s = of(agent.project_id);
      s.issues = (s.issues || []).filter(i => i.number !== number);
    });
  }

  function seg<T>(label: string, options: { label: string; value: T }[], current: T, choose: (value: T) => void) {
    return [h("span", "lbl", label), segmented(options, current, choose)];
  }

  /// The agent and machine picker under the issue being started.
  function pickRow(agent: Agent, issue: IssueRow, where: readonly Where[] | null | undefined) {
    const s = of(agent.project_id);
    if (hosts === undefined) loadHosts();
    const c = workChoices(hosts, where);
    const p = s.pick!;
    p.agent ??= c.agent;
    if (p.machine === undefined) p.machine = c.machine;
    const cancel = () => button("Cancel", "", () => { s.pick = null; redraw(); });
    if (!c.agents.length) {
      return h("span", "pj-start", h("span", "dmuted", hosts === null ? "Reading your machines…" : "No machine offers a coding agent."), cancel());
    }
    const busy = s.busy.has(`i${issue.number}`);
    const go = button(busy ? "Starting…" : "Start worker", "pri", () => track(agent, issue.number, { agent: p.agent, machine: p.machine }));
    go.disabled = !p.agent || busy;
    return h("span", "pj-start",
      ...seg("Agent", c.agents.map(a => ({ label: agentLabel(a), value: a })), p.agent, v => { p.agent = v; redraw(); }),
      ...seg<string | null | undefined>("Machine", [...c.machines.map(m => ({ label: m.name, value: m.id })), { label: "Pick at start", value: null }], p.machine, v => { p.machine = v; redraw(); }),
      h("span", "acts", go, cancel()));
  }

  function link(text: string, url: string) {
    const a = h("a", "", text);
    a.href = "#";
    a.addEventListener("click", e => { e.preventDefault(); openUrl(url); });
    return a;
  }

  /// The full Issues list.
  function issuesPanel(agent: Agent, where: readonly Where[] | null | undefined) {
    const s = of(agent.project_id);
    if (s.issues === undefined) load(agent.project_id);
    const out: HTMLElement[] = [];
    if (s.error) out.push(h("div", "cc-err", s.error));
    const list = s.issues;
    if (list === undefined) return [...out, emptyPanel(h("span", "", "Reading the issues…"))];
    if (list === null) return [...out, emptyPanel(h("b", "", "The server needs an update before it lists issues."))];
    const head = h("div", "cc-sec", h("h4", "", "Issues", h("small", "", list.length ? `${list.length} not tracked yet` : "")),
      button("Refresh", "quiet small", () => load(agent.project_id)));
    if (!list.length) return [...out, head, emptyPanel(h("span", "", "Every open issue is a task or hidden here."))];
    const rows = list.map(i => {
      const busy = s.busy.has(`i${i.number}`);
      const picking = s.pick?.number === i.number;
      const row = h("div", `pj-task pj-issue${picking ? " ask" : ""}`,
        h("span", "t", link(`#${i.number}`, i.url), ` ${i.title}`),
        h("span", "m", issueMeta(i, ago)));
      if (picking) row.append(pickRow(agent, i, where));
      else {
        const acts = [
          button("Work on it", "pri", () => { s.pick = { number: i.number }; redraw(); }),
          button("Plan", "", () => track(agent, i.number, null)),
          button(s.busy.has(`d${i.number}`) ? "Opening…" : "Discuss", "", () => discuss(agent, i.number)),
          button("Not now", "", () => dismiss(agent, i.number)),
        ];
        for (const b of acts) b.disabled = busy;
        row.append(h("span", "acts", ...acts));
      }
      return row;
    });
    out.push(head, h("div", "cc-panel", ...rows),
      h("small", "dmuted", "Not now hides an issue here; it stays open on the host. Issue text is the author's, and the agent treats it as data."));
    return out;
  }

  /// The user's pull requests and issues, with replies and chats.
  function threadsPanel(agent: Agent) {
    const s = of(agent.project_id);
    if (s.threads === undefined) load(agent.project_id);
    const list = s.threads;
    const out: HTMLElement[] = [];
    if (list === undefined) return [emptyPanel(h("span", "", "Reading your pull requests and issues…"))];
    if (list === null) return [];
    if (s.error) out.push(h("div", "cc-err", s.error));
    const waiting = list.filter(t => t.needs_reply).length;
    out.push(h("div", "cc-sec", h("h4", "", "Your pull requests and issues", h("small", "", waiting ? `${waiting} ${waiting === 1 ? "needs" : "need"} a reply` : "")),
      button("Refresh", "quiet small", () => load(agent.project_id))));
    if (!list.length) {
      out.push(emptyPanel(h("span", "", "You have no open pull requests or issues here.")));
    } else {
      out.push(h("div", "cc-panel", ...list.map(t => {
        const st = t.needs_reply ? stateChip("blocked", "needs reply") : stateChip("done", "nothing to answer");
        const row = h("div", "pj-task pj-issue", h("span", "t", link(threadCode(t), t.url), ` ${t.title}`), st, h("span", "m", threadMeta(t, ago)));
        const acts = h("span", "acts");
        if (t.needs_reply) {
          acts.append(button(s.busy.has(`r${t.number}`) ? "Asking…" : "Let the agent reply", "pri", () => act(agent.project_id, `r${t.number}`, async () => {
            openChat(await invoke<string>("center_reply_thread", { projectId: agent.project_id, repo: t.repo, number: t.number }));
          })));
        }
        acts.append(button(s.busy.has(`d${t.number}`) ? "Opening…" : "Discuss", "", () => discuss(agent, t.number)),
          button("Open", "", () => openUrl(t.url)));
        row.append(acts);
        return row;
      })));
      out.push(h("small", "dmuted", "The agent drafts the reply and nothing posts until you approve its card."));
    }
    if (s.chats?.length) {
      out.push(h("div", "cc-sec", h("h4", "", "Chats about one thread")),
        h("div", "cc-panel", ...s.chats.map(c => {
          const row = h("div", "pj-task", h("span", "t", c.title || threadCode(c)));
          row.append(h("span", "acts", button("Open chat", "", () => openChat(c.chat_id))));
          return row;
        })));
    }
    return out;
  }

  return {
    load,
    issues: (id: string) => of(id).issues,
    threads: (id: string) => of(id).threads,
    prs: (id: string) => of(id).prs,
    issuesPanel,
    threadsPanel,
  };
}
