// Projects as the window lists them, and what the "Import a project" sheet
// shows, as pure functions so they can be tested without Tauri.
// importproject.js draws the sheet; center.js draws the page and sidebar.

/// The last folder of a path.
function base(path) { return String(path || "").split(/[\\/]/).filter(Boolean).pop() || ""; }

/// Whether `cwd` is `folder` or inside it.
function within(cwd, folder) {
  const f = String(folder || "").replace(/[\\/]+$/, "");
  return !!f && (cwd === f || String(cwd || "").startsWith(f + "/"));
}

/// Imported projects and the folders agents run in, one row per project:
/// `{ name, url, machines: [{ id, name, folder }], agents, last, imported }`.
/// An agent counts for an imported project when it runs in its folder on
/// that machine; any other agent's folder is a project of its own, by its
/// last folder name. Imported first, by name, then the others by name.
export function mergeProjects(imported, running, found = []) {
  const rows = new Map();
  for (const p of imported || []) {
    const row = rows.get(p.name) || { name: p.name, url: p.url, machines: [], agents: 0, last: null, imported: true };
    row.machines.push({ id: p.machine_id, name: p.machine_name, folder: p.folder });
    rows.set(p.name, row);
  }
  const agents = (running || []).filter(r => r.kind === "agent");
  for (const r of agents) {
    let row = [...rows.values()].find(p => p.imported && p.machines.some(m => m.id === r.machine_id && within(r.cwd, m.folder)));
    if (!row) {
      const name = foundName(found, r) || base(r.cwd);
      if (!name) continue;
      row = rows.get(name);
      if (!row) {
        row = { name, url: null, machines: [], agents: 0, last: null, imported: false };
        rows.set(name, row);
      }
      if (!row.machines.some(m => m.id === r.machine_id && m.folder === r.cwd)) {
        row.machines.push({ id: r.machine_id, name: r.machine_name, folder: r.cwd });
      }
    }
    row.agents += 1;
    const at = r.last_activity || r.started_at || null;
    if (at && (!row.last || at > row.last)) row.last = at;
  }
  return [...rows.values()].sort((a, b) => (b.imported - a.imported) || a.name.localeCompare(b.name));
}

/// "org/repo" for the checkout a session runs in, from the remote of the
/// found folder that holds it; null when no found folder does or it has no remote.
function foundName(found, r) {
  for (const row of found || []) {
    if (!row.remote || !(row.places || []).some(p => p.machine_id === r.machine_id && within(r.cwd, p.folder))) continue;
    const parts = String(row.remote).trim().replace(/\.git\/?$/, "").split(/[/:]/).filter(Boolean);
    if (parts.length >= 2) return parts.slice(-2).join("/");
  }
  return null;
}

/// The rows the page shows: all, or only `name` once one is picked.
export function filterProjects(rows, name) {
  return name ? rows.filter(r => r.name === name) : rows;
}

/// "org/repo", a GitHub URL, a Forgejo URL or `git@host:org/repo`: what the
/// server takes. It says why when it is none of these.
export function looksLikeRepo(text) {
  const t = String(text || "").trim();
  if (!t) return false;
  if (/^git@[^\s:]+:[^\s/]+\/[^\s/]+$/.test(t)) return true;
  if (/^(https?|ssh):\/\/[^\s/]+\/[^\s/]+\/[^\s/]+/.test(t)) return true;
  return /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+\/?$/.test(t) || /^[a-z0-9.-]+\.[a-z]+\/[^\s/]+\/[^\s/]+/i.test(t);
}

/// The chips under the field: Auto, then each machine online.
export function machineChips(machines) {
  const online = (machines || []).filter(m => m.online);
  return [{ id: null, label: "Auto" }, ...online.map(m => ({ id: m.id, label: m.name }))];
}

/// Children for `replaceChildren` or `append` without the empty slots a
/// conditional leaves (`null`, `undefined`, `false`), which the DOM would
/// print as text.
export function present(kids) {
  return (kids || []).filter(k => k != null && k !== false);
}

/// `org/repo` for a GitHub remote, so the field reads as if typed; any other
/// remote as it is.
export function repoFromRemote(remote) {
  const r = String(remote || "").trim();
  const m = r.match(/^(?:https?:\/\/(?:www\.)?github\.com\/|ssh:\/\/git@github\.com\/|git@github\.com:)([^\s/]+)\/([^\s/]+?)(?:\.git)?\/?$/i);
  return m ? `${m[1]}/${m[2]}` : r;
}

/// The org and the name of what the field holds: ["org", "repo"], or null
/// before it names a repository.
function orgAndName(repo) {
  if (!looksLikeRepo(repo)) return null;
  const parts = String(repo).trim().replace(/\.git\/?$/, "").split(/[/:]/).filter(Boolean);
  return parts.length >= 2 ? parts.slice(-2) : null;
}

/// Where the server puts it when no folder is chosen: `projects/org/repo`
/// in the machine's workspace on Linux, `Projects/org/repo` in its home
/// elsewhere. `base` is the folder `list_machine_folder` starts in (the
/// workspace, else home); null before it is known.
export function defaultFolder(base, os, repo) {
  if (!base) return null;
  const [org, name] = orgAndName(repo) || ["org", "repo"];
  const projects = os === "linux" ? "projects" : "Projects";
  return `${base.replace(/\/+$/, "")}/${projects}/${org}/${name}`;
}

/// What importing into the folder `listing` does, as the line under it says.
export function folderNote(listing, repo) {
  if (!listing) return null;
  if (listing.origin != null) {
    const what = listing.origin ? repoFromRemote(listing.origin) : "a repository with no origin remote";
    return `A checkout of ${what}. It is imported in place, without cloning.`;
  }
  if (!listing.exists) return "Not there yet: the clone makes it.";
  if (listing.empty) return "Empty: the clone goes here.";
  const name = (orgAndName(repo) || [])[1] || "repo";
  return `Has other files: it goes in ${listing.path.replace(/\/+$/, "")}/${name}.`;
}

/// The help line under the folder.
export function folderHelp(state, machines) {
  const tokens = "Private repositories use your tokens from Settings > Credentials.";
  if (!state.machine) return `Beep picks a machine and its default folder. ${tokens}`;
  if (state.chosen) return tokens;
  const m = (machines || []).find(x => x.id === state.machine);
  return `The default on ${m ? m.name : "that machine"}. An empty folder takes the clone; one with other files gets it in a subfolder. ${tokens}`;
}

/// The sheet before anything is typed. `chosen` is the folder picked, as
/// `list_machine_folder` read it (null for the machine's default), and
/// `browsing` the folder the browser shows while it is open.
export const START = { step: "form", repo: "", machine: null, error: null, chosen: null, browsing: null };

/// The next state after `event`:
/// - `{ type: "edit", repo }` as the field changes
/// - `{ type: "pick", machine }` for a chip, null for Auto
/// - `{ type: "browse", listing }` opens or moves the folder browser
/// - `{ type: "choose", listing }` picks that folder; a checkout fills the
///   field from its origin
/// - `{ type: "unchoose" }` goes back to the default folder, and closes the
///   browser
/// - `{ type: "oops", message }` when reading a folder failed
/// - `{ type: "submit" }` from Import or Enter
/// - `{ type: "done", project }` when the server answered
/// - `{ type: "failed", message }` when it refused
export function next(state, event) {
  switch (event.type) {
    case "edit":
      if (state.step !== "form") return state;
      return { ...state, repo: event.repo, error: null };
    case "pick":
      if (state.step !== "form") return state;
      if ((event.machine ?? null) === state.machine) return state;
      return { ...state, machine: event.machine ?? null, chosen: null, browsing: null, error: null };
    case "browse":
      if (state.step !== "form" || !state.machine) return state;
      return { ...state, browsing: event.listing, error: null };
    case "choose": {
      if (state.step !== "form" || !state.machine || !event.listing) return state;
      const origin = event.listing.origin;
      const repo = origin ? repoFromRemote(origin) : state.repo;
      return { ...state, chosen: event.listing, browsing: null, repo, error: null };
    }
    case "unchoose":
      if (state.step !== "form") return state;
      return { ...state, chosen: null, browsing: null };
    case "oops":
      if (state.step !== "form") return state;
      return { ...state, error: cleanError(event.message) };
    case "submit":
      if (state.step !== "form" || !looksLikeRepo(state.repo)) return state;
      return { ...state, step: "cloning", repo: state.repo.trim(), error: null };
    case "done":
      if (state.step !== "cloning") return state;
      return { ...state, step: "done", project: event.project };
    case "failed":
      if (state.step !== "cloning") return state;
      return { ...state, step: "form", error: cleanError(event.message) };
    default:
      return state;
  }
}

/// The server's sentence, without what the bridge puts before it.
export function cleanError(message) {
  const m = String(message || "").replace(/^(bad request|error):\s*/i, "").trim();
  return m || "The import failed.";
}

/// What shows while it clones: "Cloning org/repo onto build-box…", or
/// "Importing" for a checkout already there.
export function progressText(state, machines) {
  const m = (machines || []).find(x => x.id === state.machine);
  const how = state.chosen && state.chosen.origin != null ? "Importing" : "Cloning";
  return `${how} ${state.repo} onto ${m ? m.name : "your machine"}…`;
}

/// The line under the title once it is in: where it went.
export function doneText(project) {
  const how = project.cloned ? "Cloned" : "Already there, now listed";
  return `${how} on ${project.machine_name} in ${project.folder}`;
}

// ---------- Add a project, and one project's view ----------

/// What each coding agent is called on screen.
const AGENT_NAMES = { claude: "Claude Code", codex: "Codex", pi: "pi", flsh: "flsh", gemini: "Gemini CLI", opencode: "OpenCode" };

/// "Claude Code" for `claude`; an agent Beep does not know reads as named.
export function agentLabel(agent) { return AGENT_NAMES[agent] || String(agent || ""); }

/// How many sessions a found row's agents ran, together.
export function foundSessions(row) {
  return (row?.agents || []).reduce((n, a) => n + (a.sessions || 0), 0);
}

/// The imports a found row makes: its folder on each machine, in place.
export function importPlan(row) {
  return (row?.places || []).map(p => ({ repo: row.remote, machineId: p.machine_id, folder: p.folder }));
}

/// The line while the Add a project sheet waits on the machines.
export function lookingText(machines) {
  const n = (machines || []).filter(m => m.online).length;
  const on = n === 1 ? "1 machine" : n ? `${n} machines` : "your machines";
  return `Looking through Claude Code, Codex, pi, flsh and OpenCode sessions on ${on}…`;
}

/// The coding agents running now in one of `project`'s folders, or below one,
/// on the machine that folder is on.
export function runningIn(project, running) {
  const places = project?.machines || [];
  return (running || []).filter(r => r.kind === "agent"
    && places.some(m => m.id === r.machine_id && within(r.cwd, m.folder)));
}

/// A project's past sessions without those running now (they show under
/// Agents), newest first.
export function pastSessions(sessions, running) {
  const live = new Set((running || []).map(r => `${r.machine_id} ${r.id}`));
  return [...(sessions || [])]
    .filter(s => !live.has(`${s.machine_id} ${s.id}`))
    .sort((a, b) => (b.last_activity || 0) - (a.last_activity || 0));
}

/// A project's session list after a read: `got`, else (`got` null, the
/// read failed) the list shown before, else none. `changed` says whether it
/// differs from `before`, the list shown now, so an unchanged one is not
/// drawn again.
export function nextSessions(before, got) {
  const list = got ?? before ?? [];
  return { list, changed: before === undefined || JSON.stringify(before) !== JSON.stringify(list) };
}

/// "Open on GitHub", or "Open on <host>" for another host's web page; null
/// without one.
export function openOnLabel(url) {
  let host = "";
  try { host = new URL(url).host; } catch { return null; }
  return host === "github.com" ? "Open on GitHub" : `Open on ${host}`;
}

/// When a project was first imported, in words: "today", "yesterday",
/// "3 days ago", "5 weeks ago". `at` and `now` are in milliseconds.
export function importedWhen(at, now) {
  const midnight = ms => new Date(ms).setHours(0, 0, 0, 0);
  const days = Math.round((midnight(now) - midnight(at)) / 86400000);
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  if (days < 14) return `${days} days ago`;
  return `${Math.round(days / 7)} weeks ago`;
}

/// The line under a project's name: its host and when it was imported, or
/// where it came from when it is not imported. `imported` is the server's
/// imported rows for every project.
export function projectLine(project, imported, now) {
  if (!project) return "";
  if (!project.imported) return "From running agents";
  let host = "";
  try { host = new URL(project.url).host; } catch {}
  const times = (imported || []).filter(r => r.name === project.name && r.imported_at).map(r => r.imported_at);
  const when = times.length ? `imported ${importedWhen(Math.min(...times), now)}` : "";
  return [host, when].filter(Boolean).join(" · ");
}

/// The projects pinned to the top of the sidebar, in the order they were
/// pinned, and the rest of `rows`. Each keeps `i`, its place in `rows`, so
/// its colour stays when it is pinned. A pinned name that is no longer a
/// project is left out.
export function splitOnTop(rows, top) {
  const all = (rows || []).map((p, i) => ({ ...p, i }));
  const byName = new Map(all.map(p => [p.name, p]));
  const names = [...new Set(top || [])];
  return {
    onTop: names.filter(n => byName.has(n)).map(n => byName.get(n)),
    rest: all.filter(p => !names.includes(p.name)),
  };
}

/// The sidebar's project rows grouped by GitHub org or user: an `{ org }`
/// heading, then that org's projects with `repo` set to the name after the
/// slash. Projects without an org come last, under no heading. Each keeps
/// `i`, its place in `rows` (or the `i` it has), so its colour does not
/// change with grouping.
export function sideProjects(rows) {
  const orgs = new Map(), loose = [];
  (rows || []).forEach((p, at) => {
    const i = p.i ?? at;
    const cut = p.name.indexOf("/");
    if (cut <= 0) return loose.push({ ...p, i });
    const org = p.name.slice(0, cut);
    if (!orgs.has(org)) orgs.set(org, []);
    orgs.get(org).push({ ...p, i, repo: p.name.slice(cut + 1) });
  });
  return [...[...orgs].flatMap(([org, list]) => [{ org }, ...list]), ...loose];
}

/// The rows of a project's "Where it lives": every machine, as
/// `project_machines` gives them (null from a server too old to say, when
/// the imported folders are listed), each with `kind`:
/// - "here": a checkout in `folder`; `mac` when it is this Mac
/// - "checking": the server is looking through it now
/// - "none": online without a checkout, so it can be cloned there
/// - "offline": offline without a checkout; it is looked through once it
///   connects
export function whereRows(project, machines, macId) {
  const here = (id, name, folder) => ({ kind: "here", id, name, folder, mac: !!macId && id === macId });
  if (!machines) return (project?.machines || []).map(m => here(m.id, m.name, m.folder));
  return machines.map(m => {
    if (m.folder) return here(m.id, m.name, m.folder);
    const one = { id: m.id, name: m.name, folder: null, mac: false, checked_at: m.checked_at || null };
    if (m.checking) return { ...one, kind: "checking", text: "Checking…" };
    if (m.online) return { ...one, kind: "none", text: `Not on ${m.name}` };
    return { ...one, kind: "offline", text: `Checks when ${m.name} is online` };
  });
}

/// "2 of 5 machines" for the section's heading, or "2 machines" while
/// only the imported folders are known.
export function whereCount(rows, known) {
  const n = rows.filter(r => r.kind === "here").length;
  const machines = x => x === 1 ? "1 machine" : `${x} machines`;
  return known && rows.length > n ? `${n} of ${machines(rows.length)}` : machines(n);
}

// ---------- The Projects overview ----------

/// The order a card's bar takes, work in hand first.
export const BAR_STATES = ["running", "review", "blocked", "planned", "done"];

/// The overview's cards grouped by org, as the sidebar groups projects:
/// `[{ org, cards }]`, projects without an org last under `org: null`.
/// Each card is the project row with `i` for its colour, `pinned`, and
/// `sum`, the server's overview of it (null when it has no project agent
/// yet). Pinned projects come first in their org.
export function overviewGroups(rows, overview, top) {
  const sums = new Map((overview || []).map(o => [o.name, o]));
  const pins = new Set(top || []);
  const groups = new Map();
  (rows || []).forEach((p, i) => {
    const cut = p.name.indexOf("/");
    const org = cut > 0 ? p.name.slice(0, cut) : null;
    if (!groups.has(org)) groups.set(org, []);
    groups.get(org).push({ ...p, i, repo: cut > 0 ? p.name.slice(cut + 1) : p.name, pinned: pins.has(p.name), sum: sums.get(p.name) || null });
  });
  const out = [...groups].map(([org, cards]) => ({ org, cards: cards.sort((a, b) => (b.pinned - a.pinned) || (a.i - b.i)) }));
  return [...out.filter(g => g.org), ...out.filter(g => !g.org)];
}

/// Tasks not done or dropped.
export function openTasks(counts) {
  const c = counts || {};
  return (c.running || 0) + (c.review || 0) + (c.blocked || 0) + (c.planned || 0);
}

/// The bar's segments, `{ state, share }` with share in percent, empty
/// states left out.
export function barSegments(counts) {
  const total = openTasks(counts) + ((counts && counts.done) || 0);
  if (!total) return [];
  return BAR_STATES.filter(k => counts[k]).map(k => ({ state: k, n: counts[k], share: counts[k] / total * 100 }));
}

/// "2 open · 1 in review · 1 blocked · 3 done", the parts after "open"
/// only when there are some.
export function countsLine(counts) {
  const c = counts || {};
  const parts = [`${openTasks(c)} open`];
  if (c.review) parts.push(`${c.review} in review`);
  if (c.blocked) parts.push(`${c.blocked} blocked`);
  parts.push(`${c.done || 0} done`);
  return parts;
}

/// What the task that changed last says about the project.
export function latestLine(latest) {
  if (!latest) return "No tasks yet";
  const t = latest.title || latest.slug;
  switch (latest.state) {
    case "running": return `Working on ${t}`;
    case "blocked": return `${latest.slug || t} is blocked`;
    case "review": return latest.pr_url ? `${t}: its PR waits for your review` : `${t} waits for your review`;
    case "planned": return `Next: ${t}`;
    case "done": return `Finished ${t}`;
    default: return t;
  }
}

/// "1 worker", "2 workers", or "idle".
export function workersLabel(n) {
  return n ? `${n} ${n === 1 ? "worker" : "workers"}` : "idle";
}

/// The heading's line: "3 projects in 2 orgs, 3 workers running".
export function overviewSub(groups) {
  const cards = groups.flatMap(g => g.cards);
  const orgs = groups.filter(g => g.org).length;
  const workers = cards.reduce((n, c) => n + (c.sum?.workers || 0), 0);
  const projects = `${cards.length} ${cards.length === 1 ? "project" : "projects"}`;
  const inOrgs = orgs ? ` in ${orgs} ${orgs === 1 ? "org" : "orgs"}` : "";
  return `${projects}${inOrgs}, ${workers ? `${workers} ${workers === 1 ? "worker" : "workers"} running` : "no workers running"}`;
}
