// The Instructions view's words and sums, as pure functions so they can be
// tested without Tauri. instructions.ts draws the view.

/// One saved version of the user's instructions (VersionRow).
export interface VersionRow {
  version: number;
  text: string;
  note: string;
  /// In milliseconds since the Unix epoch.
  saved_at: number;
}

/// The longest instructions Beep takes, in bytes.
export const MAX_BYTES = 16 * 1024;

/// Bytes of `text` as UTF-8.
export function bytes(text: string | null | undefined) { return new TextEncoder().encode(String(text || "")).length; }

/// "Markdown · 0.6 of 16 KB".
export function sizeLabel(text: string) {
  const kb = bytes(text) / 1024;
  return `Markdown · ${kb < 10 ? kb.toFixed(1) : Math.round(kb)} of ${MAX_BYTES / 1024} KB`;
}

/// "Version 7 · Restored version 3", the line over the editor and on a
/// History row.
export function versionLine(v: Pick<VersionRow, "version" | "note"> | null | undefined) {
  if (!v) return "Not saved yet";
  return [`Version ${v.version}`, v.note].filter(Boolean).join(" · ");
}

/// How Beep supplies the saved instructions to agents.
export function mineLayer(newest: Pick<VersionRow, "version"> | null | undefined) {
  if (!newest) return "Nothing saved yet, so agents get only the other layers";
  return `Version ${newest.version}, supplied when Beep starts or resumes a session`;
}

/// A ready-made rule the user can add to their instructions.
export interface Suggestion {
  title: string;
  /// One line on what it asks of agents.
  about: string;
  text: string;
}

export const SUGGESTIONS: readonly Suggestion[] = [{
  title: "Linear git history",
  about: "Rebase the branch, then fast-forward: no merge commits",
  text: "## Git\n\n- Keep history linear: no merge commits. To merge a feature branch, rebase it onto the main branch, rerun its tests, then `git merge --ff-only`. Only rebase a local, unpushed branch, never the main branch or anything pushed.",
}];

/// `text` with `suggestion` at the end, after a blank line.
export function withSuggestion(text: string, suggestion: Suggestion) {
  const kept = text.trimEnd();
  return kept ? `${kept}\n\n${suggestion.text}\n` : `${suggestion.text}\n`;
}
