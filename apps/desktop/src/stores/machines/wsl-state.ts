// WSL distributions of Windows machines, whose coding agents the Windows
// node runs on its own checkouts, as pure functions so they can be tested
// without Tauri.
import { harnessName } from "../settings/providers-state.ts";
import type { MachineView, WslDistro } from "../../parts/machines/machine-state.ts";

/// The Windows machines among `machines` that reported WSL distributions.
export function wslHosts<T extends Pick<MachineView, "env">>(machines: readonly T[]): T[] {
  return machines.filter(m => m.env?.os === "windows" && m.env.wsl.length > 0);
}

/// "Claude Code, Codex": the coding agents found in a distribution.
export function agentsLine(distro: Pick<WslDistro, "agents" | "probed">) {
  if (!distro.probed) return "Not looked into yet";
  return distro.agents.length ? distro.agents.map(harnessName).join(", ") : "No coding agents found";
}
