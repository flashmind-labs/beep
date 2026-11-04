// A coding agent's thought as it streams. These are the parts without the
// page.

/// A thought's Markdown as plain text for the live view: no bold, code or
/// heading marks, and at most one blank line in a row.
export function liveText(md: string) {
  return md
    .replace(/\*\*|__|`/g, "")
    .replace(/^[ \t]*#{1,6}[ \t]+/gm, "")
    .replace(/\n{3,}/g, "\n\n")
    .trimStart();
}
