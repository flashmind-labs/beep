// The sheets that add a machine or a project, and the found checkouts the Projects page lists.
import { invoke } from "../ui/tauri.ts";
import { projectChanged } from "./barstate.ts";
import { reason } from "../ui/format.ts";
import { single } from "../ui/active.ts";
import type { FoundRow } from "./projects-state.ts";
import { state, pages, sessionsDue, homeDataChanged } from "../center/state.ts";
import { go, openScreen } from "../center/nav.ts";

/// Opens the Add a machine sheet; the machines topic brings the new one.
export function openAddMachine() {
  openScreen({ screen: "addmachine" });
}

/// Opens the Projects page, filtered to `name` when given.
export function openProjects(name?: string | null) {
  return go("projects", { project: name || null });
}

/// Opens a project once it is in: the projects topic brings Home, and the found folders load again.
export function landed(p: { name: string }) {
  state.picked = p.name;
  sessionsDue.add(p.name);
  pages.drop("where", p.name);
  loadFound();
  openProjects(p.name);
}

/// Opens the Import a project sheet; the project is selected once it is in.
export function importSheet(repo: string | null = null, machine: string | null = null) {
  openScreen({ screen: "importproject", repo, machine });
}

/// Opens the Add a project sheet: found folders, a folder, a clone, or a new project.
export function addProject() {
  openScreen({ screen: "addproject" });
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
