import { AG, hostChanges, loadAgentHosts } from "../agents/agents.ts";
import { renderSettings } from "../settings/settings.ts";
import { renderWaiting } from "./waiting.ts";

/// Shows the bar on `view`, as Rust names it, or on the sessions waiting for the user.
export function show(view: string | null) {
  // Hosts stand until machines or accounts change.
  if (!AG.hosts || hostChanges.dirty("")) loadAgentHosts();
  if (view === "settings") return renderSettings();
  return renderWaiting();
}
