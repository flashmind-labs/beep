import type { AgentToolCall } from "./agents.ts";
import { esc } from "../ui/format.ts";
import { commandText, resultText } from "./tool-details-state.ts";

/// Expand a command to what the harness ran and what it returned.
export function toolDetailsHtml(call: AgentToolCall | null | undefined, status: string, live: boolean, output: string, renderedOutput?: string) {
  const command = call?.input_truncated ? call.input : commandText(call);
  return `<div class="call-details"><dl><dt>Tool</dt><dd>${esc(call?.name || "Tool name unavailable")}</dd></dl>`
    + `<h4>${call?.input_truncated ? "Arguments (truncated)" : "Command"}</h4><pre>${esc(command ?? "Command unavailable.")}</pre>`
    + `<h4>Output${!output && call?.result_truncated ? " (truncated)" : ""}</h4>${renderedOutput ?? `<pre>${esc(output || resultText(call) || (status === "pending" || live ? "No output yet." : "No output recorded."))}</pre>`}</div>`;
}
