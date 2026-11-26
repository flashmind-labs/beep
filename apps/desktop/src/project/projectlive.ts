// Whether the project page shown updates live or by polling, and the reads
// it starts.
import type { invoke as Invoke } from "../ui/tauri.ts";
import { pushedView, type Pushed, type ViewSnapshot } from "../ui/snapshot-state.ts";

/// What the desktop says of a followed project (LiveRow).
export interface LiveRow {
  project_id: string;
  live: boolean;
  polling_why: string | null;
  lists: boolean;
  runs: boolean;
  views?: ViewSnapshot[];
}

/// How a project reads its host.
export interface LiveMode {
  live: boolean;
  why: string | null;
}

/// What an update changed of a project, with the views it pushed.
interface ProjectUpdate {
  lists: boolean;
  runs: boolean;
  /// The pushed view `command` read with `args`, null when the update did not carry it.
  view<T>(command: string, args: Record<string, unknown>): Pushed<T> | null;
}

interface LiveDeps {
  invoke: typeof Invoke;
  redraw: () => void;
  /// Takes what the project's lists or runs changed to.
  reload: (id: string, update: ProjectUpdate) => void;
}

/// The imported project whose forge data this window shows right now: the open
/// project page, or the project rail beside one of its sessions.
export function followedProject(
  shown: boolean,
  view: string | null,
  picked: string | null,
  rail: string | null,
  projects: readonly { name: string; imported: boolean }[] | null,
): string | null {
  if (!shown) return null;
  const name = view === "projects" && picked ? picked : rail;
  if (!name) return null;
  return projects?.some(row => row.name === name && row.imported) ? name : null;
}

export function makeProjectLive(deps: LiveDeps) {
  const { invoke, redraw, reload } = deps;
  const modes = new Map<string, LiveMode>();
  let current: string | null = null;
  let pending = Promise.resolve();
  // A failed reply may still have registered a source. Keep its cleanup until acknowledged.
  const owned = new Set<string>();

  async function detach(id: string) {
    try {
      await invoke("unfollow_project", { projectId: id });
      owned.delete(id);
    } catch (error) {
      console.warn("unfollowing project:", error);
    }
  }

  /// Follows `id`, the project the window shows, or none.
  function follow(id: string | null): Promise<void> {
    if (id === current && [...owned].every(was => was === id)) return pending;
    current = id;
    pending = pending.then(async () => {
      for (const was of owned) if (was !== id) await detach(was);
      if (!id || owned.has(id)) return;
      owned.add(id);

      try {
        await invoke("follow_project", { projectId: id });
      } catch (error) {
        console.warn("following project:", error);
        await detach(id);
      }
    });
    return pending;
  }

  /// What the desktop said of `row.project_id`.
  function heard(row: LiveRow) {
    const id = row.project_id;
    if (id !== current) return;
    modes.set(id, { live: row.live, why: row.polling_why });
    if (row.lists || row.runs) reload(id, { lists: row.lists, runs: row.runs, view: (command, args) => pushedView(row, command, args) });
    redraw();
  }

  return {
    follow,
    heard,
    mode: (id: string): LiveMode | null => modes.get(id) ?? null,
  };
}
