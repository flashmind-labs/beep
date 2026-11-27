import type { AgentToolCall } from "../../stores/agents/agents-state.ts";
import { diffBlocks } from "./agentturns.ts";
import { commandText, resultText } from "../../stores/agents/tool-details-state.ts";

/// What an opened command shows: the tool, what the harness ran and what it
/// returned. Output with diffs shows them as changed lines.
export interface ToolDetails {
  tool: string;
  truncated: boolean;
  command: string;
  outputTruncated: boolean;
  output: { kind: "diffs"; output: string } | { kind: "text"; text: string };
}

export function toolDetails(call: AgentToolCall | null | undefined, status: string, live: boolean, output: string): ToolDetails {
  const command = call?.input_truncated ? call.input : commandText(call);
  return {
    tool: call?.name || "Tool name unavailable",
    truncated: !!call?.input_truncated,
    command: command ?? "Command unavailable.",
    outputTruncated: !output && !!call?.result_truncated,
    output: diffBlocks(output).length ? { kind: "diffs", output }
      : { kind: "text", text: output || resultText(call) || (status === "pending" || live ? "No output yet." : "No output recorded.") },
  };
}
