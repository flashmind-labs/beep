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
