// What the "Add a machine" sheet shows, as pure functions so they can be
// tested without Tauri. addmachine.js draws it and talks to the app.

/// The command that puts beep-node on a machine.
export const INSTALL = "curl -fsSL https://releases.flashmind.com/beep/node/install.sh | sh";

/// The sheet before its token arrives.
export const START = { step: "loading" };

/// The next state after `event`:
/// - `{ type: "created", token, command, expiresAt }` from create_node_token
/// - `{ type: "failed", message }` when it was refused
/// - `{ type: "tick", now }` every second, which expires a waiting token
/// - `{ type: "found", machine }` when the machine linked
/// - `{ type: "renew" }` for a new token
export function next(state, event) {
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
export function outdated(message) {
  return /needs an update/i.test(String(message || ""));
}

/// The time left until `expiresAt` as `9:41`, never below `0:00`.
export function timeLeft(expiresAt, now) {
  const secs = Math.max(0, Math.ceil((expiresAt - now) / 1000));
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
}

/// The machine with `id` in `machines`, as node_token_machine names the one
/// that claimed the token; null until the list has it.
export function machineById(machines, id) {
  if (!id) return null;
  return (machines || []).find(m => m.id === id) || null;
}

const OS_NAMES = { linux: "Linux", macos: "macOS", windows: "Windows" };
const TOOL_NAMES = {
  docker: "Docker", claude: "Claude Code", codex: "Codex", gemini: "Gemini CLI",
  xcodebuild: "Xcode", adb: "Android SDK", node: "Node", cargo: "Rust", python3: "Python", go: "Go",
};

/// "Linux, Docker, Claude Code, Codex": the OS and the known tools a
/// machine reported. Empty before its first report.
export function describeMachine(machine) {
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
