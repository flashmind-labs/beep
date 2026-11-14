import { AG, hostChanges, loadAgentHosts } from "../agents/agents.ts";
import { renderSettings } from "../settings/settings.ts";
import { renderWaiting } from "./waiting.ts";

/// Hosts stand until machines or accounts change.
export function loadHosts() {
  if (!AG.hosts || hostChanges.dirty("")) loadAgentHosts();
}

/// Shows the bar on `view`, as Rust names it, or on the sessions waiting for the user.
export function show(view: string | null) {
  loadHosts();
  if (view === "settings") return renderSettings();
  return renderWaiting();
}
