import { ensureAgentHosts } from "../../stores/agents/agents-state.ts";
import { renderSettings } from "../../screens/settings/settings.tsx";
import { renderWaiting } from "../../parts/bar/waiting.ts";

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
