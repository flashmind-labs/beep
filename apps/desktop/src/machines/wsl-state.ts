// WSL distributions of Windows machines, which each run as a machine of their
// own once added, as pure functions so they can be tested without Tauri.
import { harnessName } from "../settings/providers-state.ts";
import type { MachineView, WslDistro } from "./machine-state.ts";

/// Where a distribution stands: not added; kept and not in the mesh yet (its
/// node starting, or why it does not run); in the mesh as `name` and kept
/// running (`problem` when it is not running now); or in the mesh and off.
export type WslState =
  | { step: "add" }
  | { step: "starting"; problem: string | null }
  | { step: "running"; name: string; problem: string | null }
  | { step: "off"; name: string };

/// What the window is doing for a distribution: a step under way, done
/// adding until its machine shows, or why it failed.
export type WslWork =
  | { step: "busy"; text: string }
  | { step: "added" }
  | { step: "failed"; error: string };

/// The Windows machines among `machines` that reported WSL distributions;
/// only the one with id `only` when given.
export function wslHosts<T extends Pick<MachineView, "id" | "env">>(machines: readonly T[], only: string | null): T[] {
  return machines.filter(m => m.env?.os === "windows" && m.env.wsl.length > 0 && (only === null || m.id === only));
}

/// Where `distro` stands, with `machines` naming the machine its node joined as.
export function wslState(distro: WslDistro, machines: readonly Pick<MachineView, "id" | "name">[]): WslState {
  const joined = distro.machine_id === null ? undefined : machines.find(m => m.id === distro.machine_id);
  if (joined) return distro.kept ? { step: "running", name: joined.name, problem: distro.problem } : { step: "off", name: joined.name };
  return distro.kept ? { step: "starting", problem: distro.problem } : { step: "add" };
}

/// "Claude Code, Codex": the coding agents found in a distribution.
export function agentsLine(distro: Pick<WslDistro, "agents" | "probed">) {
  if (!distro.probed) return "Not looked into yet";
  return distro.agents.length ? distro.agents.map(harnessName).join(", ") : "No coding agents found";
}

/// The key of `distro` on machine `host` in the window's work.
export const wslKey = (host: string, distro: string) => `${host} ${distro}`;

/// The work under way for each distribution, kept apart from the rows so it
/// outlives a sheet or page that closes.
export class WslWorkStore {
  private work: ReadonlyMap<string, WslWork> = new Map();
  private readonly watchers = new Set<() => void>();

  readonly snapshot = () => this.work;

  readonly subscribe = (watch: () => void) => {
    this.watchers.add(watch);
    return () => { this.watchers.delete(watch); };
  };

  /// Notes `next` for `key`, or that nothing is under way.
  set(key: string, next: WslWork | null) {
    const changed = new Map(this.work);
    if (next) changed.set(key, next);
    else changed.delete(key);
    this.work = changed;
    for (const watch of this.watchers) watch();
  }
}

/// What the rows ask of Windows machine `host`: add `distro` as a machine,
/// turn its node off or on, or look into every distribution. `work` holds
/// how each goes.
export interface WslActions {
  work: WslWorkStore;
  add(host: string, distro: string): void;
  stop(host: string, distro: string): void;
  start(host: string, distro: string): void;
  probe(host: string): void;
}
