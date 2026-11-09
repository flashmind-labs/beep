import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { bar } from "./core.ts";

interface PageRoot { host: HTMLElement; root: Root; observer: MutationObserver; release: () => void }
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
export function openBarPage(release: () => void): { host: HTMLElement; root: Root } {
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
  const root = createRoot(host);
  const observer = new MutationObserver(() => { if (current?.host === host && !host.isConnected) dispose(); });
  observer.observe(document.body, { childList: true, subtree: true });
  current = { host, root, observer, release };
  return current;
}
