import { createDocumentScope } from "./document-root.tsx";

// An error banner: the first paragraph of a message on one line, the rest
// (a command's output, say) behind Details in a scrolling box, and a button
// that dismisses it.

const banners = createDocumentScope<string>();
let next = 0;

/// A background failure remains visible until its own dismiss button is used.
export function showFailure(message: string) {
  const key = String(++next), failure = splitFailure(message);
  banners.set([{ slot: key, host: document.body, content: <div className="fb-err" role="alert" style={{ position: "fixed", inset: "auto 16px 16px", zIndex: 1000, background: "var(--ink)" }}><div className="fb-line"><span>{failure.line}</span><button type="button" className="fb-x" title="Dismiss" aria-label="Dismiss" onClick={() => banners.remove([key])}>×</button></div>{failure.details && <details className="fb-more"><summary>Details</summary><pre>{failure.details}</pre></details>}</div> }]);
}

/// The line to show and the details below it, split at the first blank line.
export function splitFailure(message: string): { line: string; details: string } {
  const at = message.search(/\r?\n\s*\n/);
  if (at < 0) return { line: message.replace(/\s+/g, " ").trim(), details: "" };
  return { line: message.slice(0, at).replace(/\s+/g, " ").trim(), details: message.slice(at).trim() };
}
