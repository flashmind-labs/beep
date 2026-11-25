// The sheets that add a machine or a project, and the found checkouts the Projects page lists.
import { invoke } from "../ui/tauri.ts";
import { projectChanged } from "./barstate.ts";
import { reason } from "../ui/format.ts";
import { single } from "../ui/active.ts";
import { MACHINES } from "../machines/machine-store.ts";
import type { FoundRow } from "./projects-state.ts";
import { state, pages, sessionsDue, homeDataChanged } from "../center/state.ts";
import { go } from "../center/nav.ts";

/// Opens the Add a machine sheet; the machines topic brings the new one.
export function openAddMachine() {
  void import("../machines/addmachine.tsx").then(m => m.openAddMachine());
}

/// Opens the Projects page, filtered to `name` when given.
export function openProjects(name?: string | null) {
  return go("projects", { project: name || null });
}

/// Opens a project once it is in: the projects topic brings Home, and the found folders load again.
function landed(p: { name: string }) {
  state.picked = p.name;
  sessionsDue.add(p.name);
  pages.drop("where", p.name);
  loadFound();
  openProjects(p.name);
}

/// Opens the Import a project sheet; the project is selected once it is in.
export function importSheet(repo: string | null = null, machine: string | null = null) {
  void import("../addproject/importproject.tsx").then(m => m.openImportProject({
    machines: state.home?.machines || [],
    macId: MACHINES.mac?.id || null,
    repo,
    machine,
    onDone: landed,
  }));
}

/// Opens the Add a project sheet: found folders, a folder, a clone, or a new project.
export function addProject() {
  void import("../addproject/addproject.tsx").then(m => m.openAddProject({
    machines: (state.home?.machines || []).filter(mac => mac.can_host !== false),
    onImported: landed,
    onChoose: () => void import("../addproject/addfolder.tsx").then(f => f.openAddFolder({
      machines: state.home?.machines || [],
      macId: MACHINES.mac?.id || null,
      onDone: landed,
      onBack: addProject,
    })),
    onClone: repo => importSheet(repo ?? null),
    onNew: () => void import("../addproject/newproject.tsx").then(n => n.openNewProject({
      machines: state.home?.machines || [],
      onDone: landed,
      onBack: addProject,
      onClone: repo => importSheet(repo),
    })),
  }));
}

/// The checkouts the machines named last, for the Projects page, then each
/// machine's look again as it answers; it redraws each time.
export const loadFound = single(async () => {
  try {
    state.found = await invoke<FoundRow[]>("found_projects");
  } catch (e) {
    console.warn("found projects:", e);
    state.found = state.found || [];
  }

  drawFound();
  const { rescanAll } = await import("../addproject/addproject.tsx");
  await rescanAll(() => {}, (_, after) => {
    if (!after.rows) return;
    state.found = after.rows;
    drawFound();
  });
});

/// The Projects overview and a first start Home, which list the found checkouts.
function drawFound() {
  if (!state.picked) projectChanged.notify();
  homeDataChanged.notify();
}

/// Imports a found row from the Projects page and opens it.
export async function importFromPage(row: FoundRow) {
  if (state.importing) return;
  state.importing = row.name;
  state.foundError = null;
  projectChanged.notify();
  homeDataChanged.notify();

  try {
    const { importFound } = await import("../addproject/addproject.tsx");
    const project = await importFound(row);
    state.importing = null;
    landed(project);
  } catch (e) {
    state.importing = null;
    state.foundError = { name: row.name, message: reason(e) };
    drawFound();
  }
}
