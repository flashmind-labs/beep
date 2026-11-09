import { flushSync } from "react-dom";
import type { ReactNode } from "react";
import { createDocumentView } from "../ui/document-root.tsx";
import { bar } from "./core.ts";

interface DocumentRenderer { render(content: ReactNode): void; unmount(): void }
interface PageRoot { host: HTMLElement; root: DocumentRenderer; observer: MutationObserver; release: () => void }
let current: PageRoot | null = null;

function dispose() {
  const old = current;
  current = null;
  if (!old) return;
  old.observer.disconnect();
  old.release();
  old.root.unmount();
}

/// Reuses the mounted bar root when moving between React pages.
// Release the old page before rendering its replacement. Callers invalidate
// pending requests and dispose native resources here; the callback must also
// work after an imperative page has detached the host.
export function openBarPage(release: () => void): { host: HTMLElement; root: DocumentRenderer } {
  if (current && current.host.parentElement !== bar) dispose();
  if (current) {
    current.release();
    flushSync(() => current?.root.render(null));
    current.release = release;
    return current;
  }
  const host = document.createElement("div");
  host.style.display = "contents";
  host.dataset.barPage = "";
  bar.replaceChildren(host);
  const root = createDocumentView(host);
  const observer = new MutationObserver(() => { if (current?.host === host && !host.isConnected) dispose(); });
  observer.observe(document.body, { childList: true, subtree: true });
  current = { host, root, observer, release };
  return current;
}
