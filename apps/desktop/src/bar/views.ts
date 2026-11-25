import { ensureAgentHosts } from "../agents/agents-state.ts";
import { renderSettings } from "../settings/settings.tsx";
import { renderWaiting } from "./waiting.ts";

/// Hosts follow the pushes; the first read is made here.
export function loadHosts() {
  void ensureAgentHosts();
}

/// Shows the bar on `view`, as Rust names it, or on the sessions waiting for the user.
export function show(view: string | null) {
  loadHosts();
  if (view === "settings") return renderSettings();
  return renderWaiting();
}
