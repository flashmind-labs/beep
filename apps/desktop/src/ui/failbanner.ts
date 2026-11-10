// An error banner: the first paragraph of `message` on one line, the rest
// (a command's output, say) behind Details in a scrolling box, and a
// button that dismisses it.
/// The line to show and the details below it, split at the first blank line.
export function splitFailure(message: string): { line: string; details: string } {
  const at = message.search(/\r?\n\s*\n/);
  if (at < 0) return { line: message.replace(/\s+/g, " ").trim(), details: "" };
  return { line: message.slice(0, at).replace(/\s+/g, " ").trim(), details: message.slice(at).trim() };
}
