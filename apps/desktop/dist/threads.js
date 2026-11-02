// A project's issues and the user's pull requests and issues there. The
// board's Issues tile and the Pull requests tile's rows, and their full
// lists: an open issue no task tracks gets Work on it (agent and machine),
// Plan, Discuss and Not now; a thread of the user's gets Let the agent
// reply when someone wrote after them, and Discuss. center.js draws the
// page; this file keeps the state.
import { agentLabel } from "./projects-state.js";

/// "#52", "PR #41".
export function threadCode(t) {
  return t.kind === "pr" ? `PR #${t.number}` : `#${t.number}`;
}

/// The Issues tile from the open issues no task tracks; undefined while
/// they load, null from a server too old to list them.
export function issueTile(list) {
  if (list === undefined) return { count: "", rows: [], empty: "Loading…" };
  if (list === null) return { count: "", rows: [], empty: "The server lists no issues yet" };
  return {
    count: list.length ? `${list.length} untracked` : "",
    rows: list.map(i => ({ code: `#${i.number}`, text: i.title, url: i.url, note: i.labels?.[0] || (i.comments ? `${i.comments} comments` : "") })),
    empty: "No untracked issues",
  };
}

/// The Pull requests tile from the user's threads: their open PRs, those
/// waiting on a reply first; `fallback` (the tasks' PRs) while they load or
/// on a server too old to list them.
export function threadTile(list, fallback) {
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
export function issueMeta(i, ago) {
  const out = [`opened by ${i.author || "someone"}`];
  if (i.opened_at) out.push(ago(i.opened_at));
  out.push(...(i.labels || []));
  if (i.comments) out.push(`${i.comments} ${i.comments === 1 ? "comment" : "comments"}`);
  return out.join(" · ");
}

/// The line under a thread: who wrote last, else that nothing waits.
export function threadMeta(t, ago) {
  const out = [];
  if (t.last_other) out.push(t.last_other);
  if (t.updated_at) out.push(ago(t.updated_at));
  if (!t.needs_reply) out.push("your last comment is the newest");
  return out.join(" · ");
}

/// What Work on it offers: the agents the machines offer, in the order
/// first seen, and the machines with the project, then "Pick at start".
/// The first agent offered is picked.
export function workChoices(hosts, where) {
  const agents = [];
  for (const h of hosts || []) for (const a of h.agents || []) if (!agents.includes(a)) agents.push(a);
  const here = new Set((where || []).filter(r => r.kind === "here").map(r => r.id));
  const machines = (hosts || []).filter(h => here.has(h.machine_id)).map(h => ({ id: h.machine_id, name: h.machine_name, agents: h.agents || [] }));
  return { agents, machines, agent: agents[0] || null, machine: machines[0]?.id || null };
}

/// `deps`: { invoke, h, button, ago, redraw, openUrl, openChat, changed }.
/// `openChat(chatId)` shows a chat in the pane; `changed()` reloads the
/// project's tasks once an issue became one.
export function makeThreads(deps) {
  const { invoke, h, button, ago, redraw, openUrl, openChat, changed } = deps;
  // By project id: { issues, threads, chats, error, busy, pick }.
  const all = new Map();
  let hosts;

  const of = id => {
    if (!all.has(id)) all.set(id, { issues: undefined, threads: undefined, chats: undefined, error: "", busy: new Set(), pick: null });
    return all.get(id);
  };
  const fail = e => String(e).replace(/^(bad request|error):\s*/i, "");

  async function load(id) {
    const s = of(id);
    const [issues, threads, chats] = await Promise.allSettled([
      invoke("center_project_issues", { projectId: id }),
      invoke("center_project_threads", { projectId: id }),
      invoke("center_thread_chats", { projectId: id }),
    ]);
    s.error = "";
    for (const [key, r] of [["issues", issues], ["threads", threads], ["chats", chats]]) {
      if (r.status === "fulfilled") s[key] = r.value;
      else { s[key] = s[key] ?? []; s.error = fail(r.reason); }
    }
    redraw();
  }

  async function loadHosts() {
    if (hosts !== undefined) return;
    hosts = null;
    try { hosts = await invoke("agent_hosts"); } catch (e) { console.warn("agent hosts:", e); hosts = []; }
    redraw();
  }

  /// Runs `work` for row `key`, showing it busy and its error.
  async function act(id, key, work) {
    const s = of(id);
    if (s.busy.has(key)) return;
    s.busy.add(key);
    s.error = "";
    redraw();
    try {
      await work();
    } catch (e) {
      s.error = fail(e);
    } finally {
      s.busy.delete(key);
      redraw();
    }
  }

  const discuss = (agent, number) => act(agent.project_id, `d${number}`, async () => {
    const chat = await invoke("center_open_thread_chat", { projectId: agent.project_id, repo: agent.name, number });
    const s = of(agent.project_id);
    s.chats = [chat, ...(s.chats || []).filter(c => c.chat_id !== chat.chat_id)];
    openChat(chat.chat_id);
  });

  function track(agent, number, start) {
    return act(agent.project_id, `i${number}`, async () => {
      await invoke("center_track_issue", { projectId: agent.project_id, number, agent: start?.agent ?? null, nodeId: start?.machine ?? null });
      const s = of(agent.project_id);
      s.issues = (s.issues || []).filter(i => i.number !== number);
      s.pick = null;
      changed();
    });
  }

  function dismiss(agent, number) {
    return act(agent.project_id, `i${number}`, async () => {
      await invoke("center_dismiss_issue", { projectId: agent.project_id, number });
      const s = of(agent.project_id);
      s.issues = (s.issues || []).filter(i => i.number !== number);
    });
  }

  function seg(label, options, current, choose) {
    const box = h("span", "pb-seg");
    for (const o of options) {
      const b = h("button", "", o.label);
      b.type = "button";
      b.addEventListener("click", () => choose(o.value));
      b.setAttribute("aria-pressed", String(o.value === current));
      box.append(b);
    }
    return [h("span", "lbl", label), box];
  }

  /// The agent and machine picker under the issue being started.
  function pickRow(agent, issue, where) {
    const s = of(agent.project_id);
    if (hosts === undefined) loadHosts();
    const c = workChoices(hosts, where);
    const p = s.pick;
    p.agent ??= c.agent;
    if (p.machine === undefined) p.machine = c.machine;
    if (!c.agents.length) {
      return h("span", "pj-start", h("span", "dmuted", hosts === null ? "Reading your machines…" : "No machine offers a coding agent."),
        button("Cancel", "", () => { s.pick = null; redraw(); }));
    }
    const go = button(s.busy.has(`i${issue.number}`) ? "Starting…" : "Start worker", "pri", () => track(agent, issue.number, { agent: p.agent, machine: p.machine }));
    go.disabled = !p.agent || s.busy.has(`i${issue.number}`);
    return h("span", "pj-start",
      ...seg("Agent", c.agents.map(a => ({ label: agentLabel(a), value: a })), p.agent, v => { p.agent = v; redraw(); }),
      ...seg("Machine", [...c.machines.map(m => ({ label: m.name, value: m.id })), { label: "Pick at start", value: null }], p.machine, v => { p.machine = v; redraw(); }),
      h("span", "acts", go, button("Cancel", "", () => { s.pick = null; redraw(); })));
  }

  function link(text, url) {
    const a = h("a", "", text);
    a.href = "#";
    a.addEventListener("click", e => { e.preventDefault(); openUrl(url); });
    return a;
  }

  /// The full Issues list.
  function issuesPanel(agent, where) {
    const s = of(agent.project_id);
    if (s.issues === undefined) load(agent.project_id);
    const out = [];
    if (s.error) out.push(h("div", "cc-err", s.error));
    const list = s.issues;
    if (list === undefined) return [...out, h("div", "cc-panel cc-empty", h("span", "", "Reading the issues…"))];
    if (list === null) return [...out, h("div", "cc-panel cc-empty", h("b", "", "The server needs an update before it lists issues."))];
    const head = h("div", "cc-sec", h("h4", "", "Issues", h("small", "", list.length ? `${list.length} not tracked yet` : "")),
      button("Refresh", "quiet small", () => load(agent.project_id)));
    if (!list.length) return [...out, head, h("div", "cc-panel cc-empty", h("span", "", "Every open issue is a task or hidden here."))];
    const rows = list.map(i => {
      const busy = s.busy.has(`i${i.number}`);
      const picking = s.pick?.number === i.number;
      const row = h("div", `pj-task pj-issue${picking ? " ask" : ""}`,
        h("span", "t", link(`#${i.number}`, i.url), ` ${i.title}`),
        h("span", "m", issueMeta(i, ago)));
      if (picking) row.append(pickRow(agent, i, where));
      else {
        const acts = h("span", "acts",
          button("Work on it", "pri", () => { s.pick = { number: i.number }; redraw(); }),
          button("Plan", "", () => track(agent, i.number, null)),
          button(s.busy.has(`d${i.number}`) ? "Opening…" : "Discuss", "", () => discuss(agent, i.number)),
          button("Not now", "", () => dismiss(agent, i.number)));
        for (const b of acts.children) b.disabled = busy;
        row.append(acts);
      }
      return row;
    });
    out.push(head, h("div", "cc-panel", ...rows),
      h("small", "dmuted", "Not now hides an issue here; it stays open on the host. Issue text is the author's, and the agent treats it as data."));
    return out;
  }

  /// The user's pull requests and issues, with replies and chats.
  function threadsPanel(agent) {
    const s = of(agent.project_id);
    if (s.threads === undefined) load(agent.project_id);
    const list = s.threads;
    const out = [];
    if (list === undefined) return [h("div", "cc-panel cc-empty", h("span", "", "Reading your pull requests and issues…"))];
    if (list === null) return [];
    if (s.error) out.push(h("div", "cc-err", s.error));
    const waiting = list.filter(t => t.needs_reply).length;
    out.push(h("div", "cc-sec", h("h4", "", "Your pull requests and issues", h("small", "", waiting ? `${waiting} ${waiting === 1 ? "needs" : "need"} a reply` : "")),
      button("Refresh", "quiet small", () => load(agent.project_id))));
    if (!list.length) {
      out.push(h("div", "cc-panel cc-empty", h("span", "", "You have no open pull requests or issues here.")));
    } else {
      out.push(h("div", "cc-panel", ...list.map(t => {
        const st = t.needs_reply ? h("span", "pj-st blocked", "needs reply") : h("span", "pj-st done", "nothing to answer");
        const row = h("div", "pj-task pj-issue", h("span", "t", link(threadCode(t), t.url), ` ${t.title}`), st, h("span", "m", threadMeta(t, ago)));
        const acts = h("span", "acts");
        if (t.needs_reply) {
          acts.append(button(s.busy.has(`r${t.number}`) ? "Asking…" : "Let the agent reply", "pri", () => act(agent.project_id, `r${t.number}`, async () => {
            const chat = await invoke("center_reply_thread", { projectId: agent.project_id, repo: t.repo, number: t.number });
            openChat(chat);
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
    issues: id => of(id).issues,
    threads: id => of(id).threads,
    issuesPanel,
    threadsPanel,
  };
}
