// Where a session works, as its machine says (`WorkPlace`): the checkout, and
// whether it is a linked worktree. The sub line of a session row names it.

import { base } from "../ui/format.ts";
import { remotePath } from "./running-state.ts";

/// A session's checkout, as `agent_place` gives it.
export interface WorkPlace {
  /// The top of the checkout holding the folder.
  folder: string;
  /// The repository's main folder.
  main: string;
  /// A linked worktree, not the main checkout.
  linked: boolean;
  /// The branch checked out; null on a detached head.
  branch: string | null;
  /// The `origin` remote's URL, empty when none.
  origin: string;
}

/// What names a session for `agent_place`.
export interface PlaceAsk {
  machine_id: string;
  agent: string;
  id: string;
  external?: boolean;
}

export const placeKey = (r: PlaceAsk) => `${r.machine_id}:${r.agent}:${r.id}`;

/// What the checkout is called: its branch, else its folder.
export const placeName = (p: WorkPlace) => p.branch ?? base(p.folder);

/// The repository: `org/repo` from its remote, else its folder's name.
export const placeRepo = (p: WorkPlace) => remotePath(p.origin) ?? base(p.main);

/// The sub line of a session row in a linked worktree: `org/repo · name ·
/// machine`. Null for one in the main checkout or one not known.
export function placeSub(p: WorkPlace | null | undefined, machine: string) {
  if (!p?.linked) return null;
  return [placeRepo(p), placeName(p), machine].filter(Boolean).join(" · ");
}
