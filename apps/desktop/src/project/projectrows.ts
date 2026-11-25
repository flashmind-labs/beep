// Imported projects merged with the folders agents run in.
import { mergeProjects, type Project } from "./projects-state.ts";
import { state } from "../center/state.ts";
import type { HomeView } from "../center/home-state.ts";

/// Imported projects merged with the folders agents run in.
export function projectRows() {
  const home = state.home;
  if (!home || (!home.running && !home.projects)) return null;
  // Home, the sidebar and the pages all ask on each draw; the answer only
  // changes with the lists it is made from.
  if (merged?.projects !== home.projects || merged.running !== home.running) {
    merged = { projects: home.projects, running: home.running, rows: mergeProjects(home.projects, home.running) };
  }
  return merged.rows;
}
let merged: { projects: HomeView["projects"]; running: HomeView["running"]; rows: Project[] } | undefined;
