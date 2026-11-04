// What the "Add a machine" sheet shows, as pure functions so they can be
// tested without Tauri. addmachine.ts draws it and talks to the app.
import type { MacControl, MachineEnv, MachineView } from "../center/home-state.ts";

/// The command that puts beep-node on a machine.
export const INSTALL = "curl -fsSL https://releases.flashmind.com/beep/node/install.sh | sh";

/// Where a new machine looks for projects unless told otherwise.
export const HOME_SEARCH = "~/";

/// The search folder as the node takes it: empty for home.
export function searchFolder(typed: string) {
  const dir = typed.trim();
  return dir === "~" || dir === HOME_SEARCH ? "" : dir;
}

/// `command` with `--search <dir>` when the folder is not home, quoted
/// for the shell when it has to be.
export function withSearch(command: string, typed: string) {
  const dir = searchFolder(typed);
  if (!dir) return command;
  return `${command} --search ${/^[\w~./-]+$/.test(dir) ? dir : `'${dir.replace(/'/g, `'\\''`)}'`}`;
}

/// A machine the sheet names, as `list_machines` gives it.
type Machine = Pick<MachineView, "id" | "name"> & { env?: Pick<MachineEnv, "os" | "tools"> | null };

/// The sheet's state: loading a token, waiting for a machine to claim it,
/// linked, or why it stopped.
export type AddState =
  | { step: "loading" }
  | { step: "waiting"; token: string; command: string; expiresAt: number }
  | { step: "linked"; machine: Machine }
  | { step: "expired" }
  | { step: "outdated" | "error"; message: string };

/// What changes the sheet's state; see `next`.
export type AddEvent =
  | { type: "created"; token: string; command: string; expiresAt: number }
  | { type: "failed"; message: string }
  | { type: "tick"; now: number }
  | { type: "found"; machine: Machine }
  | { type: "renew" };

/// The sheet before its token arrives.
export const START: AddState = { step: "loading" };

/// The next state after `event`:
/// - `created` from create_node_token
/// - `failed` when it was refused
/// - `tick` every second, which expires a waiting token
/// - `found` when the machine linked
/// - `renew` for a new token
export function next(state: AddState, event: AddEvent): AddState {
  switch (event.type) {
    case "created":
      if (state.step !== "loading") return state;
      return { step: "waiting", token: event.token, command: event.command, expiresAt: event.expiresAt };
    case "failed":
      if (state.step !== "loading") return state;
      return { step: outdated(event.message) ? "outdated" : "error", message: event.message };
    case "tick":
      if (state.step === "waiting" && event.now >= state.expiresAt) return { step: "expired" };
      return state;
    case "found":
      if (state.step !== "waiting") return state;
      return { step: "linked", machine: event.machine };
    case "renew":
      return START;
    default:
      return state;
  }
}

/// Whether a refusal says the server is too old for setup tokens.
export function outdated(message: string | null | undefined) {
  return /needs an update/i.test(String(message || ""));
}

/// The time left until `expiresAt` as `9:41`, never below `0:00`.
export function timeLeft(expiresAt: number, now: number) {
  const secs = Math.max(0, Math.ceil((expiresAt - now) / 1000));
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
}

/// The machine with `id` in `machines`, as node_token_machine names the one
/// that claimed the token; null until the list has it.
export function machineById<T extends { id: string }>(machines: readonly T[] | null | undefined, id: string | null | undefined) {
  if (!id) return null;
  return (machines || []).find(m => m.id === id) || null;
}

const OS_NAMES: Record<string, string> = { linux: "Linux", macos: "macOS", windows: "Windows" };
const TOOL_NAMES: Record<string, string> = {
  docker: "Docker", claude: "Claude Code", codex: "Codex", gemini: "Gemini CLI", opencode: "OpenCode",
  xcodebuild: "Xcode", adb: "Android SDK", node: "Node", cargo: "Rust", python3: "Python", go: "Go",
};

/// "Linux, Docker, Claude Code, Codex": the OS and the known tools a
/// machine reported. Empty before its first report.
export function describeMachine(machine: { env?: { os: string; tools?: readonly { name: string }[] } | null } | null | undefined) {
  const env = machine && machine.env;
  if (!env) return "";
  const parts = [];
  const os = OS_NAMES[env.os] || env.os;
  if (os) parts.push(os);
  for (const tool of env.tools || []) {
    const name = TOOL_NAMES[tool.name];
    if (name && !parts.includes(name)) parts.push(name);
  }
  return parts.join(", ");
}

/// What the sheet knows of this Mac: its mac_control view, or after a failed
/// try to turn it on, what that view had with the failure.
export type MacSeen = Pick<MacControl, "enabled" | "status" | "error"> & { name?: string };

/// The sheet's "Use this Mac" option: its line, and its button or how it
/// stands.
export interface MacOption {
  text: string;
  button?: string;
  done?: boolean;
  bad?: boolean;
  busy?: boolean;
}

/// The sheet's "Use this Mac" option from the app's mac_control view: a line
/// saying where this Mac stands, and a button while it is off.
export function macOption(mac: MacSeen | null | undefined, asked: boolean): MacOption | null {
  if (!mac || mac.status === "unlinked") return null;
  const name = mac.name || "This Mac";
  if (!mac.enabled) return { text: `Let Beep run commands and use the screen on ${name}.`, button: "Use this Mac" };
  switch (mac.status) {
    case "connected":
      return asked ? { text: `${name} is linked. It shows under Machines.`, done: true } : null;
    case "failed":
      return { text: `Couldn't connect: ${mac.error || "unknown error"}. Beep tries again on its own.`, bad: true };
    default:
      return { text: `Connecting ${name}`, busy: true };
  }
}
