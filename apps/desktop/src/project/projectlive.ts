// Whether the project page shown updates live (only an older server says so)
// or by polling, and the reads it starts.
import type { invoke as Invoke } from "../ui/tauri.ts";
import type { MakeEl } from "../ui/dom.ts";
import { installViews, type ViewSnapshot } from "../ui/snapshot-state.ts";

/// What the server says of a followed project (LiveRow).
export interface LiveRow {
  project_id: string;
  /// Null from a server too old to say.
  live: boolean | null;
  polling_why: string | null;
  lists: boolean;
  runs: boolean;
  views?: ViewSnapshot[];
}

/// How a project reads its host: undefined while unknown.
export interface LiveMode {
  live: boolean;
  why: string | null;
}

export interface LiveDeps {
  invoke: typeof Invoke;
  h: MakeEl;
  redraw: () => void;
  /// Reads the project's lists or runs again.
  reload: (id: string, lists: boolean, runs: boolean) => void;
}

const LIVE_TIP = "Live: updates through a webhook";
const POLLING_TIP = "Polling every few minutes. Live needs admin rights on the repository.";

/// The chip next to a section's synced time; null while unknown.
export function modeChip(h: MakeEl, mode: LiveMode | undefined) {
  if (!mode) return null;
  const chip = h("small", mode.live ? "sw-mode live" : "sw-mode", mode.live ? "live" : "polling");
  chip.title = mode.live ? LIVE_TIP : mode.why ? `Polling: ${mode.why}` : POLLING_TIP;
  return chip;
}

export function makeProjectLive(deps: LiveDeps) {
  const { invoke, h, redraw, reload } = deps;
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

  /// What the server said of `row.project_id`.
  function heard(row: LiveRow) {
    const id = row.project_id;
    if (id !== current) return;
    installViews(row);
    if (row.live === null) modes.delete(id);
    else modes.set(id, { live: row.live, why: row.polling_why });
    if (row.lists || row.runs) reload(id, row.lists, row.runs);
    redraw();
  }

  return {
    follow,
    heard,
    chip: (id: string) => modeChip(h, modes.get(id)),
  };
}
