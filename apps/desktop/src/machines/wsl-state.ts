// WSL distributions of Windows machines, which each run as a machine of their
// own once added, as pure functions so they can be tested without Tauri.
import { harnessName } from "../settings/providers-state.ts";
import type { MachineView, WslDistro } from "./machine-state.ts";

/// Where a distribution stands: not added, added and not in the mesh yet
/// (its node starting, or why it does not run), or in the mesh as `name`.
export type WslState =
  | { step: "add" }
  | { step: "starting"; problem: string | null }
  | { step: "joined"; name: string; kept: boolean; problem: string | null };

/// What the window is doing for a distribution: adding it (the step under
/// way), turning it off, done adding until its machine shows, or why it failed.
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
  if (joined) return { step: "joined", name: joined.name, kept: distro.kept, problem: distro.problem };
  return distro.kept ? { step: "starting", problem: distro.problem } : { step: "add" };
}

/// "Claude Code, Codex": the coding agents found in a distribution.
export function agentsLine(agents: readonly string[]) {
  return agents.length ? agents.map(harnessName).join(", ") : "No coding agents found";
}

/// The key of `distro` on machine `host` in the window's work.
export const wslKey = (host: string, distro: string) => `${host} ${distro}`;

/// What the rows ask of Windows machine `host`: add `distro` as a machine,
/// hearing each `step` as it begins, or turn its node off.
export interface WslActions {
  add(host: string, distro: string, step: (text: string) => void): Promise<void>;
  stop(host: string, distro: string): Promise<void>;
}
