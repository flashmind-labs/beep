// An error banner: the first paragraph of `message` on one line, the rest
// (a command's output, say) behind Details in a scrolling box, and a
// button that dismisses it.
import { button, el } from "./dom.ts";

/// The line to show and the details below it, split at the first blank line.
export function splitFailure(message: string): { line: string; details: string } {
  const at = message.search(/\r?\n\s*\n/);
  if (at < 0) return { line: message.replace(/\s+/g, " ").trim(), details: "" };
  return { line: message.slice(0, at).replace(/\s+/g, " ").trim(), details: message.slice(at).trim() };
}

export function failBanner(message: string, onDismiss: () => void): HTMLElement {
  const { line, details } = splitFailure(message);
  const close = button("×", "fb-x", () => onDismiss());
  close.title = "Dismiss";
  close.setAttribute("aria-label", "Dismiss");
  const box = el("div", "fb-err", el("div", "fb-line", el("span", "", line), close));
  box.setAttribute("role", "alert");
  if (details) {
    const more = el("details", "fb-more", el("summary", "", "Details"), el("pre", "", details));
    box.append(more);
  }
  return box;
}
