// A project's agent on its page: the Tasks list, the Workers tree and the
// project chat with its cards, or the call to start that chat. center.js
// owns the page and its refreshes; this file draws these sections and holds
// the pure pieces the tests check.
import { agentOf } from "./home.js";
import { agentLabel } from "./projects-state.js";

/// How many turns of the project chat the page shows; the chat itself has
/// the rest.
export const CHAT_TURNS = 12;

/// The project agent for project `name`, or null.
export function agentFor(agents, name) {
  return (agents || []).find(a => a.name === name) || null;
}

/// "18 min", "2 h 5 min", "3 d": how long `from` to `to` took.
export function duration(from, to) {
  const min = Math.max(0, Math.floor((to - from) / 60000));
  if (min < 1) return "under a minute";
  if (min < 60) return `${min} min`;
  const hours = Math.floor(min / 60);
  if (hours < 24) return min % 60 ? `${hours} h ${min % 60} min` : `${hours} h`;
  return `${Math.floor(hours / 24)} d`;
}

/// What a task row offers: Drop while it may still happen, Reopen once it
/// is over.
export function taskAction(state) {
  if (state === "dropped" || state === "done") return { change: "reopen", label: "Reopen" };
  return { change: "drop", label: "Drop" };
}

/// The section's count line: "2 running · 1 in review · 3 planned".
export function taskSummary(tasks) {
  const words = { running: "running", review: "in review", blocked: "blocked", planned: "planned", done: "done", dropped: "dropped" };
  const counts = new Map();
  for (const t of tasks || []) counts.set(t.state, (counts.get(t.state) || 0) + 1);
  return Object.keys(words).filter(k => counts.get(k)).map(k => `${counts.get(k)} ${words[k]}`).join(" · ");
}

/// "PR #41" from a pull request's address, "PR" when it has no number.
export function prLabel(url) {
  const n = String(url || "").match(/\/(?:pull|pulls)\/(\d+)/);
  return n ? `PR #${n[1]}` : "PR";
}

/// The pieces under a task's title: its issue and PR links, who works on
/// it, its branch, and the task it waits for while that is not done.
export function taskMeta(task, tasks, workers) {
  const out = [];
  if (task.issue_url) out.push({ text: task.issue_number ? `#${task.issue_number}` : "Issue", url: task.issue_url });
  if (task.pr_url) out.push({ text: prLabel(task.pr_url), url: task.pr_url });
  const w = task.worker && (workers || []).find(x => x.id === task.worker);
  if (w) out.push({ text: `${agentLabel(w.agent)} on ${w.machine_name}` });
  if (task.branch) out.push({ text: task.branch, code: true });
  const before = task.depends_on && (tasks || []).find(t => t.id === task.depends_on);
  if (before && before.state !== "done") out.push({ text: `after ${before.title}` });
  return out;
}

/// The folder a worker runs in, from `.worktrees/` on when it is a
/// worktree, else its last two parts.
export function workerFolder(folder) {
  const f = String(folder || "");
  const at = f.indexOf(".worktrees/");
  if (at >= 0) return f.slice(at);
  return f.split(/[\\/]/).filter(Boolean).slice(-2).join("/");
}

/// A worker's line: agent, machine, folder, state and how long it ran.
export function workerLine(w, now) {
  const took = duration(w.started_at, w.ended_at ?? now);
  return [agentLabel(w.agent), w.machine_name, workerFolder(w.folder), w.state, took].filter(Boolean).join(" · ");
}

/// The last `n` turns of the chat, with what is being sent after them
/// until the chat has it.
export function recentTurns(turns, sending, n = CHAT_TURNS) {
  const list = (turns || []).slice(-n);
  if (!sending) return list;
  const last = list.filter(t => t.role === "user").pop();
  return last?.text === sending ? list : [...list, { role: "user", text: sending }];
}

// ---------- Drawing ----------

/// An element with `cls`; string children become text, never markup.
function h(tag, cls, ...kids) {
  const el = document.createElement(tag);
  if (cls) el.className = cls;
  for (const k of kids) if (k != null && k !== false) el.append(k);
  return el;
}

function button(label, cls, on) {
  const b = h("button", `dbtn${cls ? " " + cls : ""}`, label);
  b.type = "button";
  b.addEventListener("click", on);
  return b;
}

function section(id, title, sub, body) {
  const el = h("div");
  el.id = id;
  el.append(h("div", "cc-sec", h("h4", "", title), sub ? h("small", "", sub) : null), body);
  return el;
}

function chip(state) {
  return h("span", `pj-st ${state}`, state === "running" ? h("span", "dspin") : null, state);
}

function link(text, url, open) {
  const a = h("a", "", text);
  a.href = url;
  a.addEventListener("click", e => { e.preventDefault(); open(url); });
  return a;
}

/// The Tasks section. `act.change(task, change)` drops or reopens one.
export function tasksSection(view, act) {
  const tasks = view.tasks;
  if (!tasks.length) {
    return section("sec-tasks", "Tasks", "", h("div", "cc-panel cc-empty", h("span", "", "No tasks yet. Ask the project agent below and it plans them.")));
  }
  const rows = tasks.map(t => {
    const meta = h("span", "m", ...taskMeta(t, tasks, view.workers).map(m =>
      m.url ? link(m.text, m.url, act.openUrl) : m.code ? h("code", "", m.text) : h("span", "", m.text)));
    const { change, label } = taskAction(t.state);
    const busy = act.busy(t.id);
    const b = button(busy ? "…" : label, "quiet small", () => act.change(t, change));
    b.disabled = !!busy;
    const err = act.error(t.id);
    const title = h("span", "t", t.title);
    title.title = t.title;
    return h("div", "pj-task", title, chip(t.state), meta, h("span", "acts", b, err ? h("span", "cc-err", err) : null));
  });
  return section("sec-tasks", "Tasks", taskSummary(tasks), h("div", "cc-panel", ...rows));
}

/// The Workers tree: the project agent, then each worker under it. Worker
/// rows open nothing yet; a live one offers Stop.
export function workersSection(view, act, chatIcon) {
  const titles = new Map(view.tasks.map(t => [t.id, t.title]));
  const now = Date.now();
  const lead = (ag, kid) => h("span", `cc-ag ${ag}`, kid);
  const icon = h("span");
  icon.style.display = "inline-flex";
  icon.innerHTML = chatIcon; // fixed markup from center.js, no server text
  const nodes = [h("div", "pj-node", lead("beep", icon.firstChild), h("span", "t", h("b", "", "Project agent"), h("span", "", "Plans tasks, files issues, places workers")))];
  for (const w of view.workers) {
    const ag = agentOf(w.agent);
    const name = titles.get(w.task_id) || workerFolder(w.folder);
    const sub = h("span", "", workerLine(w, now));
    sub.title = w.folder;
    const node = h("div", "pj-node child", lead(ag.cls, ag.ini), h("span", "t", h("b", "", name), sub));
    if (w.live) {
      const busy = act.busy(w.id);
      const stop = button(busy ? "Stopping…" : "Stop", "quiet small", () => act.stop(w));
      stop.disabled = !!busy;
      const err = act.error(w.id);
      node.append(h("span", "acts", stop, err ? h("span", "cc-err", err) : null));
    }
    if (w.last_output) {
      const out = h("span", "out", w.last_output);
      out.title = w.last_output;
      node.append(out);
    }
    if (!w.live) node.classList.add("over");
    nodes.push(node);
  }
  const live = view.workers.filter(w => w.live).length;
  return section("sec-workers", "Workers", live ? `${live} running` : "none running", h("div", "cc-panel pj-tree", ...nodes));
}

/// One card the project chat waits on: what it asks, its fields, and
/// Approve or Deny.
function card(c, act) {
  const out = act.outcome(c.query_id);
  const a = c.approval;
  const items = a.fields.map(f => h("li", "", f.label, " ", h("small", "", f.value)));
  const acts = h("div", "acts");
  if (out?.said) acts.append(h("span", "ok", out.said));
  else {
    acts.append(
      button("Approve", "pri small", () => act.answer(c, "allow", "Approved")),
      button("Deny", "warn small", () => act.answer(c, "deny", "Denied")));
    if (out?.error) acts.append(h("span", "cc-err", out.error));
  }
  const el = h("div", "pj-plan", h("b", "", a.action), a.app ? h("small", "", a.app) : null, items.length ? h("ol", "", ...items) : null, acts);
  if (out?.said) el.style.opacity = ".55";
  return el;
}

/// The cards the project chat waits on, above the board; null without any.
export function cardsSection(view, act) {
  if (!view.cards.length) return null;
  return section("sec-project-needs", "Needs you", "the project chat waits on these", h("div", "pj-needs", ...view.cards.map(c => card(c, act))));
}

/// The pull requests the project's tasks opened, each linked, with its
/// task's state and branch.
export function prsSection(view, act) {
  const tasks = view.tasks.filter(t => t.pr_url && t.state !== "dropped");
  if (!tasks.length) {
    return section("sec-prs", "Pull requests", "", h("div", "cc-panel cc-empty", h("span", "", "No pull requests yet. A task's worker opens one when its work is ready.")));
  }
  const rows = tasks.map(t => {
    const title = h("span", "t", link(prLabel(t.pr_url), t.pr_url, act.openUrl), " ", t.title);
    title.title = t.title;
    return h("div", "pj-task", title, chip(t.state), h("span", "m", t.branch ? h("code", "", t.branch) : null));
  });
  const open = tasks.filter(t => t.state !== "done").length;
  return section("sec-prs", "Pull requests", open ? `${open} open` : "", h("div", "cc-panel", ...rows));
}

/// The project chat: its last turns, the cards it waits on, and a way to
/// the whole chat.
export function chatSection(view, act) {
  const turns = recentTurns(view.turns, act.sending);
  const body = h("div", "cc-panel pj-chat");
  if (!turns.length && !view.cards.length) body.append(h("span", "pj-quiet", "Nothing said yet. Ask for work below; the agent plans it into tasks."));
  for (const t of turns) {
    if (t.role === "user") body.append(h("div", "dask", t.text));
    else if (t.role === "assistant") {
      const said = h("div", "pj-said");
      said.innerHTML = t.html; // rendered and cleaned by markdown.rs
      body.append(said);
    } else if (t.role === "scheduled") body.append(h("span", "pj-quiet", t.label));
    else if (t.role === "approval") body.append(h("span", "pj-quiet", `Asked: ${t.action}`));
  }
  if (act.sending) body.append(h("span", "pj-quiet", h("span", "dspin"), " The project agent is working"));
  for (const c of view.cards) body.append(card(c, act));
  const all = button("Open the whole chat", "quiet small", act.openChat);
  const el = section("sec-project-chat", "Project chat", "", body);
  // The note and the button stay together at the right of the title.
  el.querySelector(".cc-sec").append(h("span", "cc-sec-r", h("small", "", "the project agent"), all));
  return el;
}

/// The call to start a project chat, for a project without an agent.
export function startSection(act) {
  const go = button(act.starting ? "Starting…" : "Start the project chat", "pri", act.start);
  go.disabled = !!act.starting;
  return section("sec-project-chat", "Project chat", "no project agent yet", h("div", "cc-panel cc-empty",
    h("b", "", "Hand the work to a project agent"),
    h("span", "", "It turns what you ask into tasks, runs each one as a worker on a machine with a checkout, and asks you before anything leaves the project."),
    go,
    act.error ? h("span", "cc-err", act.error) : null));
}
