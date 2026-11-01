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
export function mergeProjects(imported, running) {
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
      const name = base(r.cwd);
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

/// The sheet before anything is typed.
export const START = { step: "form", repo: "", machine: null, error: null };

/// The next state after `event`:
/// - `{ type: "edit", repo }` as the field changes
/// - `{ type: "pick", machine }` for a chip, null for Auto
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
      return { ...state, machine: event.machine ?? null };
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

/// What shows while it clones: "Cloning org/repo onto build-box…".
export function progressText(state, machines) {
  const m = (machines || []).find(x => x.id === state.machine);
  return `Cloning ${state.repo} onto ${m ? m.name : "your machine"}…`;
}

/// The line under the title once it is in: where it went.
export function doneText(project) {
  const how = project.cloned ? "Cloned" : "Already there, now listed";
  return `${how} on ${project.machine_name} in ${project.folder}`;
}
