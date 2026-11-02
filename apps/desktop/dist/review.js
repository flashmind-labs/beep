// A project's branches and the review of one before a merge. The board's
// Branches tile and its full list, each branch with Review; the review
// view: its facts, the reviewers' shards, the verified findings and the
// actions (fix with a worker, post the review, merge), polled while the
// reviewers read. center.js draws the page; this file keeps the state.
import { agentLabel } from "./projects-state.js";
import { worktreeMachines, worktreeName } from "./branchwork.js";

/// How often a running review is read again.
const POLL_MS = 4000;

/// Findings neither dismissed nor dropped by the verifier.
export function openFindings(r) {
  return (r?.findings || []).filter(f => !f.dismissed && f.verdict !== "dropped");
}

/// Whether a reviewer or a verifier still works.
export function isRunning(r) {
  return (r?.shards || []).some(s => s.state === "reading" || s.state === "verifying");
}

/// "running", "blocked" (conflicts or failing checks), "changes" (open
/// findings) or "ready".
export function reviewState(r) {
  if (isRunning(r)) return "running";
  if (r.merge === "conflicts" || r.pr?.checks === "failing") return "blocked";
  if (openFindings(r).length) return "changes";
  return "ready";
}

/// The pill's words for each state.
export const STATE_LABEL = { running: "Reviewing", blocked: "Blocked", changes: "Needs changes", ready: "Ready" };

function plural(n, one, many) { return `${n} ${n === 1 ? one : many}`; }

/// Why Merge is off, or "" when it may ask.
export function mergeBlock(r) {
  if (isRunning(r)) return "Actions open when the review is done";
  const why = [];
  if (r.pr?.checks === "failing") why.push("CI fails");
  if (r.merge === "conflicts") why.push(`${plural(r.conflicts.length, "file conflicts", "files conflict")}`);
  if (why.length) return `Merge is off while ${why.join(" and ")}`;
  const open = openFindings(r).length;
  if (open) return `Merge is off while ${plural(open, "finding is", "findings are")} open. Fix them or dismiss them.`;
  if (!r.pr) return "Merge needs a pull request for the branch";
  return "Merge always asks first. It never runs on its own.";
}

/// The facts row: changed lines, shards, CI, merge, head and when.
export function reviewFacts(r, ago) {
  const out = [
    { bold: String(r.changed_lines), text: " changed lines" },
    { bold: String(r.shards.length), text: r.shards.length === 1 ? " shard" : " shards" },
  ];
  const ci = r.pr?.checks;
  if (!r.pr) out.push({ text: "No pull request", dot: "off" });
  else if (ci === "failing") out.push({ text: "CI failing", dot: "bad", bad: true });
  else if (ci === "pending") out.push({ text: "CI running", spin: true });
  else if (ci === "passing") out.push({ text: "CI passing", dot: "" });
  else out.push({ text: "No CI reported", dot: "off" });
  if (r.merge === "conflicts") out.push({ text: `Conflicts in ${plural(r.conflicts.length, "file", "files")}`, bad: true });
  else if (r.merge === "clean") out.push({ text: "Merges cleanly" });
  else out.push({ text: "Merge not checked" });
  const since = ms => (ago(ms) === "now" ? "just now" : `${ago(ms)} ago`);
  const when = isRunning(r) ? `started ${since(r.started_at)}` : since(r.head_at);
  out.push({ code: r.head.slice(0, 7), text: ` · ${when}`, pre: "head " });
  return out;
}

/// "file:18", or the file alone.
export function findingPlace(f) {
  return f.line ? `${f.file}:${f.line}` : f.file;
}

/// A shard's second line: "268 lines · 6 files · Claude Code on build-box".
export function shardLine(s, machine) {
  const who = machine ? `${agentLabel(s.agent)} on ${machine}` : agentLabel(s.agent);
  return `${plural(s.lines, "line", "lines")} · ${plural(s.files, "file", "files")} · ${who}`;
}

/// A shard's state chip: { cls, text, spin }.
export function shardChip(s) {
  if (s.state === "reading") return { cls: "running", text: s.note || "reading", spin: true };
  if (s.state === "verifying") return { cls: "running", text: "verifying", spin: true };
  if (s.state === "failed") return { cls: "bad", text: "failed" };
  return { cls: "done", text: s.found ? `done · ${plural(s.found, "finding", "findings")}` : "done · nothing found" };
}

/// The findings section's subtitle.
export function findingsSub(r) {
  if (isRunning(r)) return "reviewing";
  const open = openFindings(r).length;
  return open ? `${open} open, ranked` : "none open";
}

/// What the empty findings panel says once the review is done.
export function readySummary(r) {
  const dropped = (r.findings || []).filter(f => f.verdict === "dropped");
  const dismissed = (r.findings || []).filter(f => f.dismissed && f.verdict !== "dropped").length;
  const parts = [`${plural(r.shards.length, "reviewer", "reviewers")} read all ${r.changed_lines} lines.`];
  if (dropped.length === 1) parts.push(`The verifier dropped 1 finding that was not real${dropped[0].dropped_why ? `: ${dropped[0].dropped_why}` : "."}`);
  else if (dropped.length) parts.push(`The verifier dropped ${dropped.length} findings that were not real.`);
  if (dismissed) parts.push(`${plural(dismissed, "finding was", "findings were")} dismissed.`);
  return parts.join(" ");
}

/// The Branches tile: branches with work first (the default branch last),
/// each with its PR, worker or how far ahead it is.
export function branchTile(data, reviews, ago) {
  if (data === undefined) return { count: "", rows: [], empty: "Loading…" };
  if (data === null) return { count: "", rows: [], empty: "The server lists no branches" };
  const byBranch = new Map((reviews || []).map(r => [r.branch, r]));
  const rows = data.branches.map(b => {
    const r = byBranch.get(b.name);
    const isDefault = b.name === data.default_branch;
    let note;
    if (isDefault) note = b.behind ? `default · ${b.behind} behind` : "default";
    else if (r) note = STATE_LABEL[reviewState(r)].toLowerCase();
    else if (b.ahead) note = `+${b.ahead}${b.worktree ? " · worktree" : ""}${b.pushed ? "" : " · not pushed"}`;
    else note = `merged · ${ago(b.committed_at)}`;
    return {
      code: b.name,
      note,
      chip: r && reviewState(r) !== "ready" && !isDefault ? { cls: reviewState(r) === "running" ? "run" : reviewState(r) === "blocked" ? "bad" : "wait", text: STATE_LABEL[reviewState(r)].toLowerCase() } : null,
      title: `${b.name}: ${b.subject}`,
      work: !isDefault && b.ahead > 0,
    };
  });
  const work = rows.filter(r => r.work).length;
  rows.sort((a, b) => Number(b.work) - Number(a.work));
  return { count: work ? `${work} with work` : data.branches.length ? String(data.branches.length) : "", rows, empty: "No branches" };
}

const TICK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5 9-10"/></svg>';
const INITIALS = { claude: "CC", codex: "CX", gemini: "GM", opencode: "OC", pi: "PI", flsh: "FL" };

/// `deps`: { invoke, h, button, ago, redraw, openUrl, openTile, machines }.
/// `machines()` gives Home's machines, for where a worktree goes.
export function makeReviews(deps) {
  const { invoke, h, button, ago, redraw, openUrl, openTile } = deps;
  const machines = deps.machines || (() => []);
  // By project: { branches, reviews, open, review, starting, error, note, busy }.
  const all = new Map();
  let timer = null;
  let polling = null;

  const of = name => {
    if (!all.has(name)) all.set(name, { branches: undefined, reviews: undefined, open: null, review: null, starting: null, error: "", note: "", busy: "" });
    return all.get(name);
  };
  const fail = e => String(e).replace(/^(bad request|error):\s*/i, "");

  async function load(name) {
    const t = of(name);
    const [branches, reviews] = await Promise.all([
      invoke("project_branches", { project: name }).catch(e => { t.error = fail(e); return t.branches ?? { default_branch: "", branches: [] }; }),
      invoke("reviews_list", { project: name }).catch(e => { console.warn("reviews:", e); return t.reviews ?? []; }),
    ]);
    t.branches = branches;
    t.reviews = reviews;
    redraw();
  }

  function keep(name, r) {
    const t = of(name);
    if (t.open === r.id) t.review = r;
    t.reviews = [r, ...(t.reviews || []).filter(x => x.branch !== r.branch)];
  }

  async function read(name) {
    const t = of(name);
    if (!t.open) return;
    const id = t.open;
    try {
      const r = await invoke("review_get", { id });
      if (t.open !== id) return;
      keep(name, r);
      t.error = "";
    } catch (e) {
      t.error = fail(e);
    }
    redraw();
  }

  /// Polls the open review of `name` while it runs and shows; null stops.
  function watch(name) {
    if (polling === name) return;
    clearInterval(timer);
    polling = name;
    if (!name) return;
    timer = setInterval(() => {
      if (document.visibilityState === "visible") read(name);
    }, POLL_MS);
  }

  /// Opens `branch`'s newest review, starting one when it has none or
  /// `again` asks for a fresh one.
  async function openBranch(name, branch, again = false) {
    const t = of(name);
    t.error = "";
    t.note = "";
    const had = !again && (t.reviews || []).find(r => r.branch === branch);
    openTile("review");
    if (had) {
      t.open = had.id;
      t.review = had;
      read(name);
      return;
    }
    t.open = null;
    t.review = null;
    t.starting = branch;
    redraw();
    try {
      const r = await invoke("review_start", { project: name, branch });
      t.open = r.id;
      keep(name, r);
    } catch (e) {
      t.error = fail(e);
    } finally {
      t.starting = null;
    }
    redraw();
  }

  async function act(name, what, call, done) {
    const t = of(name);
    t.busy = what;
    t.error = "";
    t.note = "";
    redraw();
    try {
      const out = await call();
      done(out);
    } catch (e) {
      t.error = fail(e);
    } finally {
      t.busy = "";
    }
    redraw();
  }

  const dismiss = (name, r, f) => act(name, `f${f.id}`,
    () => invoke("review_dismiss", { review: r.id, finding: f.id, dismissed: !f.dismissed }),
    out => keep(name, out));

  const fix = (name, r) => act(name, "fix", () => invoke("review_fix", { review: r.id }),
    task => { of(name).note = task.state === "running" ? `Started ${task.title}. It shows under Tasks.` : `Made ${task.title}; it starts when a worker slot frees.`; });

  const post = (name, r) => act(name, "post", () => invoke("review_post", { review: r.id }),
    () => { of(name).note = "Asked the project agent to post the review. Its approval card shows at the top of the project before anything posts."; });

  const merge = (name, r) => act(name, "merge", () => invoke("review_merge", { review: r.id }),
    () => { of(name).note = "Asked the project agent to merge. Its card shows at the top of the project; nothing merges until you approve it."; });

  // ---------- Branches ----------

  /// Gives `branch` a worktree on `machine`, then reads the branches again.
  const addWorktree = (name, branch, machine) => act(name, `wt:${branch}`,
    () => invoke("add_branch_worktree", { project: name, branch, machineId: machine.id }),
    b => { const t = of(name); t.picking = null; t.note = `Added ${b.worktree || worktreeName(branch)} on ${machine.name}.`; load(name); });

  /// A branch without a worktree: one on the machine the list was read
  /// on, or Other machine… to pick among the online ones.
  function worktreeActs(name, t, data, b) {
    const where = worktreeMachines(machines(), data.machine_id);
    const here = where.find(m => m.id === data.machine_id) || { id: data.machine_id, name: data.machine_name };
    const busy = t.busy === `wt:${b.name}`;
    if (t.picking === b.name) {
      return [
        ...where.map(m => { const x = button(m.name, "", () => addWorktree(name, b.name, m)); x.disabled = busy; return x; }),
        button("Cancel", "quiet", () => { t.picking = null; redraw(); }),
      ];
    }
    const go = button(busy ? "Adding…" : `Worktree on ${here.name}`, "", () => addWorktree(name, b.name, here));
    go.disabled = busy;
    go.title = `git worktree add ${worktreeName(b.name)}`;
    const other = where.length > 1 ? button("Other machine…", "quiet", () => { t.picking = b.name; redraw(); }) : null;
    return [go, other].filter(Boolean);
  }

  function branchesPanel(name) {
    const t = of(name);
    if (t.branches === undefined) load(name);
    const out = [];
    if (t.error) out.push(h("div", "cc-err", t.error));
    if (t.note && t.note.startsWith("Added ")) out.push(h("div", "cc-note", h("span", "ddot"), h("span", "", t.note)));
    const data = t.branches;
    if (data === null) {
      out.push(h("div", "cc-panel cc-empty", h("b", "", "The server needs an update before it lists branches.")));
      return out;
    }
    if (!data) {
      out.push(h("div", "cc-panel cc-empty", h("span", "dspin"), h("span", "", "Reading the branches…")));
      return out;
    }
    const byBranch = new Map((t.reviews || []).map(r => [r.branch, r]));
    const rows = data.branches.map(b => {
      const r = byBranch.get(b.name);
      const isDefault = b.name === data.default_branch;
      const meta = [
        isDefault ? "default" : `+${b.ahead} -${b.behind}`,
        b.worktree ? `worktree ${b.worktree}` : null,
        b.pushed ? null : "not pushed",
        `${ago(b.committed_at)}: ${b.subject}`,
      ].filter(Boolean);
      const acts = h("span", "acts rv-acts");
      if (r) acts.append(button(`Open review · ${STATE_LABEL[reviewState(r)].toLowerCase()}`, "", () => openBranch(name, b.name)));
      if (!isDefault && b.ahead > 0) acts.append(button(r ? "Review again" : "Review", r ? "" : "pri", () => openBranch(name, b.name, !!r)));
      if (!isDefault && !b.worktree) acts.append(...worktreeActs(name, t, data, b));
      const title = h("span", "t", h("code", "", b.name));
      title.title = b.subject;
      return h("div", "pj-task", title, h("span", "pj-st", isDefault ? "default" : b.local ? "local" : "origin"),
        h("span", "m", ...meta.map(x => h("span", "", x))), acts.childNodes.length ? acts : null);
    });
    out.push(h("div", "", h("div", "cc-sec", h("h4", "", "Branches"), h("small", "", `read on ${data.machine_name}`)),
      rows.length ? h("div", "cc-panel", ...rows) : h("div", "cc-panel cc-empty", h("span", "", "No branches"))));
    return out;
  }

  // ---------- Review ----------

  function fact(f) {
    const el = h("span", `rv-fact${f.bad ? " bad" : ""}`);
    if (f.spin) el.append(h("span", "dspin"));
    else if (f.dot != null) el.append(h("span", `ddot${f.dot ? " " + f.dot : ""}`));
    if (f.pre) el.append(f.pre);
    if (f.bold) el.append(h("b", "", f.bold));
    if (f.code) el.append(h("code", "", f.code));
    el.append(f.text);
    return el;
  }

  function tick() {
    const s = h("span");
    s.innerHTML = TICK; // fixed markup from this file, no server text
    return s.firstChild;
  }

  function findingRow(name, r, f) {
    const dropped = f.verdict === "dropped";
    const ver = dropped ? h("span", "")
      : f.verdict === "waiting" ? h("span", "rv-ver wait", h("span", "dspin"), "verifying")
      : f.verdict === "verified" ? h("span", "rv-ver", tick(), "Verified")
      : h("span", "rv-ver wait", "Not verified");
    const text = h("span", "t", h("b", "", f.title), dropped ? null : h("code", "", findingPlace(f)),
      h("span", "", dropped ? `Not real: ${f.dropped_why || "the verifier found no problem"}` : f.why));
    const right = h("span", "rv-side", ver);
    if (!dropped && !isRunning(r)) {
      const b = button(f.dismissed ? "Bring back" : "Dismiss", "quiet small", () => dismiss(name, r, f));
      b.disabled = of(name).busy === `f${f.id}`;
      right.append(b);
    }
    const el = h("div", `rv-find${dropped || f.dismissed ? " gone" : ""}`,
      h("span", `rv-sev ${dropped ? "" : f.severity}`, dropped ? "dropped" : f.dismissed ? "dismissed" : f.severity), text, right);
    return el;
  }

  function reviewPanel(name) {
    const t = of(name);
    const out = [];
    if (t.starting) {
      out.push(h("div", "cc-panel cc-empty", h("span", "dspin"), h("b", "", `Starting the review of ${t.starting}`),
        h("span", "", "Fetching, reading the diff and starting a read-only reviewer per shard.")));
      watch(null);
      return out;
    }
    const r = t.review;
    if (!r) {
      if (t.error) out.push(h("div", "cc-note", h("span", "ddot bad"), h("span", "cc-err", t.error)));
      out.push(h("div", "cc-panel cc-empty", h("b", "", "No review open"), button("Branches", "", () => openTile("branches"))));
      watch(null);
      return out;
    }
    const st = reviewState(r);
    const running = st === "running";
    const pill = h("span", `rv-pill ${st}`, running ? h("span", "dspin") : null, STATE_LABEL[st]);
    const tools = h("span", "rv-tools");
    if (r.pr) tools.append(button(`PR #${r.pr.number}`, "", () => openUrl(r.pr.url)));
    if (!running) {
      const again = button("Run again", "", () => openBranch(name, r.branch, true));
      again.title = "Reviews the branch head again";
      tools.append(again);
    }
    out.push(h("div", "rv-head", h("h3", "", `Review ${r.branch} into ${r.base}`), pill));
    out.push(h("div", "rv-facts", ...reviewFacts(r, ago).map(fact), tools));
    if (t.error) out.push(h("div", "cc-note", h("span", "ddot bad"), h("span", "cc-err", t.error)));
    if (t.note) out.push(h("div", "cc-note", h("span", "ddot"), h("span", "", t.note)));
    const blocked = [];
    if (r.merge === "conflicts") {
      blocked.push(h("b", "", `Conflicts with ${r.base} in ${plural(r.conflicts.length, "file", "files")}`),
        h("span", "", ...r.conflicts.slice(0, 6).flatMap((f, i) => [i ? ", " : "", h("code", "", f)]), r.conflicts.length > 6 ? ` and ${r.conflicts.length - 6} more` : ""));
    }
    if (r.pr?.checks === "failing") blocked.push(h("b", "", "CI failing"), h("span", "", `Checks fail on PR #${r.pr.number}. The review still ran.`));
    if (blocked.length) out.push(h("div", "cc-note rv-cfl", h("span", "ddot bad"), h("span", "rv-cfl-t", ...blocked)));

    const findings = r.findings || [];
    let body;
    if (findings.length) {
      body = h("div", "cc-panel", ...findings.map(f => findingRow(name, r, f)));
      if (running) body.append(h("div", "cc-empty rv-more", "More findings show as the reviewers finish."));
    } else if (running) {
      body = h("div", "cc-panel cc-empty", h("span", "dspin"), h("span", "", "Findings show as the reviewers finish."));
    } else {
      body = h("div", "cc-panel cc-empty", h("b", "", "No findings"), h("span", "rv-sum", readySummary(r)));
    }
    if (!running && findings.length && !openFindings(r).length) body.append(h("div", "rv-sum-row", readySummary(r)));
    const findSec = h("div", "", h("div", "cc-sec", h("h4", "", "Findings"), h("small", "", findingsSub(r))), body);
    const shards = h("div", "",
      h("div", "cc-sec", h("h4", "", "Shards"), h("small", "", "read-only reviewers")),
      h("div", "cc-panel", ...r.shards.map(s => {
        const c = shardChip(s);
        const chipEl = h("span", `pj-st ${c.cls}`, c.spin ? h("span", "dspin") : null, c.text);
        chipEl.title = s.note || c.text;
        return h("div", "rv-shard", h("span", `cc-ag ${s.agent}`, INITIALS[s.agent] || s.agent.slice(0, 2).toUpperCase()),
          h("span", "t", h("b", "", s.area), h("span", "", shardLine(s, r.machine_name))), chipEl);
      })),
      h("div", "rv-hint", "Each reviewer sees the full stat and its shard's diff. A fresh verifier checks every finding before it counts."));
    out.push(h("div", "pj-cols", findSec, shards));

    const open = openFindings(r).length;
    const why = mergeBlock(r);
    const fixB = button(t.busy === "fix" ? "Starting…" : "Fix with worker", open && !running ? "pri" : "", () => fix(name, r));
    fixB.disabled = running || !open || !!t.busy;
    fixB.title = open ? `Starts a task on ${r.branch} with ${plural(open, "finding", "findings")}` : "";
    const postB = button(t.busy === "post" ? "Asking…" : "Post review…", "", () => post(name, r));
    postB.disabled = running || !r.pr || !!t.busy;
    postB.title = r.pr ? "The project agent posts it; its card asks you first" : "The branch has no pull request";
    const mergeOk = !running && why.startsWith("Merge always");
    const mergeB = button(t.busy === "merge" ? "Asking…" : "Merge…", mergeOk ? "pri" : "", () => merge(name, r));
    mergeB.disabled = !mergeOk || !!t.busy;
    out.push(h("div", "rv-bar", fixB, postB, mergeB, h("small", "", why)));
    watch(running ? name : null);
    return out;
  }

  return {
    load,
    branches: name => of(name).branches,
    reviews: name => of(name).reviews,
    openBranch,
    branchesPanel,
    reviewPanel,
    stop: () => watch(null),
  };
}
