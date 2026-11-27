// A command's actual input and output, separate from its display label.
import type { AgentToolCall } from "./agents-state.ts";

function json(text: string | null | undefined): unknown {
  try { return text == null ? null : JSON.parse(text); } catch { return null; }
}

function input(call: AgentToolCall | null | undefined): Record<string, unknown> | null {
  if (call?.input_truncated) return null;
  const value = json(call?.input);
  return value != null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : null;
}

export function commandText(call: AgentToolCall | null | undefined): string | null {
  const args = input(call);
  const command = args?.command ?? args?.cmd;
  if (typeof command === "string") return command;
  if (Array.isArray(command) && command.length && command.every(arg => typeof arg === "string")) return JSON.stringify(command, null, 2);
  return null;
}

/// A known shell tool, or a call whose reported arguments carry a command.
export function isCommandCall(call: AgentToolCall | null | undefined) {
  if (!call) return false;
  return call.name ? ["bash", "shell", "exec", "exec_command", "shell_command", "terminal", "execute"].includes(call.name.toLowerCase()) : commandText(call) != null;
}

export function resultText(call: AgentToolCall | null | undefined) {
  if (!call?.result) return "";
  if (call.result_truncated) return call.result;
  const value = json(call.result);
  if (typeof value === "string") return value;

  // Claude's transcript stores tool output as text content blocks.
  if (Array.isArray(value) && value.every(item => item?.type === "text" && typeof item.text === "string")) {
    return value.map(item => item.text).join("\n");
  }

  return value == null ? call.result : JSON.stringify(value, null, 2);
}
