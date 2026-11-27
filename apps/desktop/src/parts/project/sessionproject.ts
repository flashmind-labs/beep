// The project a coding session runs in, read from the projects the window knows.
import { state } from "../../stores/center/state.ts";
import { go } from "../center/nav.ts";
import { projectOf, repoOf } from "../../stores/agents/running-state.ts";
import { nextRunning } from "./sessionsload.ts";
import { openProjects } from "./addflow.ts";

/// The project a coding session runs in, and how to open it.
export interface SessionProject {
  label: string;
  name: string;
  open(): void;
  afterClose(s: { machine_id: string; id: string; started_at?: number }): void;
}

export function sessionProject(s: { machine_id: string; cwd: string }): SessionProject | null {
  const projects = state.home?.projects ?? [], p = projectOf(s, projects);
  if (!p) return null;
  const open = () => void openProjects(p.name);
  // A closed session hands over to the project's next running one, else to the project.
  const afterClose = (closed: { machine_id: string; id: string; started_at?: number }) => {
    const next = nextRunning(p.name, closed);
    if (next) go("pane", { agent: next }); else open();
  };
  return { label: repoOf(s, projects) || p.name, name: p.name, open, afterClose };
}

/// The other projects on the session's machine it can move to.
export function sessionDestinations(s: { machine_id: string; cwd: string }): { name: string; folder: string }[] {
  const projects = state.home?.projects ?? [];
  const current = projectOf(s, projects);
  return projects
    .filter(p => p.machine_id === s.machine_id && p.name !== current?.name && p.folder !== s.cwd)
    .map(p => ({ name: p.name, folder: p.folder }));
}
