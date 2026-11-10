import { createDocumentScope } from "./document-root.tsx";
import { splitFailure } from "./failbanner.ts";

const banners = createDocumentScope<string>();
let next = 0;

/// A background failure remains visible until its own dismiss button is used.
export function showFailure(message: string) {
  const key = String(++next), failure = splitFailure(message);
  banners.set([{ slot: key, host: document.body, content: <div className="fb-err" role="alert" style={{ position: "fixed", inset: "auto 16px 16px", zIndex: 1000, background: "var(--ink)" }}><div className="fb-line"><span>{failure.line}</span><button type="button" className="fb-x" title="Dismiss" aria-label="Dismiss" onClick={() => banners.remove([key])}>×</button></div>{failure.details && <details className="fb-more"><summary>Details</summary><pre>{failure.details}</pre></details>}</div> }]);
}
