// What a machine says of the ACP adapters it installs for its agents
// (EnvReport adapters), as the Machines rows, Coding agents and the project
// composer show it.
import { harnessName } from "../settings/providers-state.ts";

/// An agent whose ACP adapter a machine is installing (`failed` null) or
/// could not install.
export interface AdapterInstall { agent: string; failed: string | null }

/// "Installing the Codex adapter" or "Could not install the Codex adapter: <why>".
export function adapterLine(a: AdapterInstall) {
  const name = harnessName(a.agent);
  return a.failed === null ? `Installing the ${name} adapter` : `Could not install the ${name} adapter: ${a.failed}`;
}

/// How `agent`'s adapter stands on a machine; undefined when nothing is pending.
export function adapterOf(adapters: AdapterInstall[] | undefined, agent: string) {
  return adapters?.find(a => a.agent === agent);
}
