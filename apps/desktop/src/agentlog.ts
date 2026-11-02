// A coding agent's tool entries, read back from the text the node writes.
// A live session writes `title [status]` and then the output's lines; a
// session Beep follows from Claude Code's transcript writes `Name: detail`,
// with ` (failed)` after one that failed.

const STATUSES = ["pending", "in_progress", "completed", "failed"];
const LIVE = new RegExp(`^([\\s\\S]*?) \\[(${STATUSES.join("|")})\\](?:\\n([\\s\\S]*))?$`);

/// How many output lines a tool entry shows until it is opened.
export const TOOL_LINES = 4;

/// The parts of a tool entry: its title, its status (one of `STATUSES`, or
/// "" when the text does not say) and its output, "" when it has none.
export function parseTool(text) {
  text = String(text ?? "");
  const live = LIVE.exec(text);
  if (live) return { title: plainTitle(live[1]), status: live[2], output: unfence(live[3] ?? "") };
  const failed = / \(failed\)$/.exec(text);
  if (failed) return { title: text.slice(0, failed.index), status: "failed", output: "" };
  return { title: text, status: "", output: "" };
}

/// Titles quote what they name in backticks (Edit `src/main.rs`); the row
/// shows them in mono already.
function plainTitle(title) {
  return title.trim().replace(/`([^`\n]*)`/g, "$1");
}

/// Output the agent wrapped in one fenced block, without the fence.
function unfence(output) {
  const m = /^(`{3,}|~{3,})[^\n`]*\n([\s\S]*?)\n?\1\s*$/.exec(output.trim());
  return (m ? m[2] : output).replace(/\s+$/, "");
}

/// The output as the row shows it: closed, its first `TOOL_LINES` lines and
/// how many more there are.
export function toolLines(output, open = false) {
  if (!output) return { shown: "", more: 0 };
  const lines = output.split("\n");
  if (open || lines.length <= TOOL_LINES) return { shown: output, more: 0 };
  return { shown: lines.slice(0, TOOL_LINES).join("\n"), more: lines.length - TOOL_LINES };
}
