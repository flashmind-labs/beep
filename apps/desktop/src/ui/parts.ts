// Shared command-center controls, dialog lifetimes, focus and clipboard behavior.
import { button, el, type Kid } from "./dom.ts";
import { createElement, createRef, type MouseEvent as ReactMouseEvent } from "react";
import { createDocumentScope } from "./document-root.tsx";

/// Fixed SVG markup as a node, never server text.
export function svgEl(markup: string) {
  // Each icon is parsed once; rows that repeat it get a copy.
  let node = parsedSvg.get(markup);

  if (node === undefined) {
    const s = el("span");
    s.innerHTML = markup;
    node = s.firstChild;
    parsedSvg.set(markup, node);
  }

  return node && node.cloneNode(true);
}
const parsedSvg = new Map<string, ChildNode | null>();

/// A `dbtn` button with `cls` that runs `on`.
export function dbtn(label: string | Node, cls: string, on: (e: MouseEvent) => void) {
  return button(label, `dbtn${cls ? " " + cls : ""}`, on);
}

/// A panel with nothing to list yet; `kids` say why.
export function emptyPanel(...kids: Kid[]) {
  return el("div", "cc-panel cc-empty", ...kids);
}

/// A section's head: its title, then notes or a button.
export function secHead(title: Kid, ...kids: Kid[]) {
  return el("div", "cc-sec", el("h4", "", title), ...kids);
}

/// `text` on the clipboard. The webview may refuse the async way; the old way
/// still works.
export async function writeClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const previous = document.activeElement;
    const t = el("textarea");
    t.value = text;
    document.body.append(t);
    try {
      t.select();
      if (!document.execCommand("copy")) throw new Error("Copy failed");
    } finally {
      t.remove();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    }
  }
}

/// Copies `text` and says so on `on` for a moment.
export async function copy(text: string, on?: HTMLElement | null) {
  await writeClipboard(text);
  if (!on) return;
  const was = on.innerHTML;
  on.textContent = "Copied";
  setTimeout(() => { if (on.isConnected) on.innerHTML = was; }, 1200);
}

/// Gives focus back to what had it before a sheet opened.
export function focusBack(was: Element | null) {
  (was as HTMLElement | null)?.focus?.();
}

type SheetOptions = {
  /// Mount directly into the caller's host so React owns removal there.
  host?: HTMLElement;
  /// False while the sheet must stay: a click on the scrim and the default Esc do nothing.
  canClose?: () => boolean;
  /// Sees every key first, ahead of the page; true when it took the key (Esc included).
  onKey?: (e: KeyboardEvent) => boolean | void;
  /// Runs once the sheet is gone.
  onClose?: () => void;
};

const openSheets: HTMLElement[] = [];

/// Whether a sheet was placed above this element.
export function sheetAbove(element: HTMLElement) {
  return openSheets.some(root => !!(element.compareDocumentPosition(root) & Node.DOCUMENT_POSITION_FOLLOWING));
}

interface SheetFrameOptions { host: HTMLElement; scrimClass: string; onScrim(): void }

/// A mounted React dialog frame. Its caller owns dismissal and focus behavior.
export function mountSheetFrame(label: string, cls: string, options: SheetFrameOptions) {
  const rootRef = createRef<HTMLDivElement>(), sheetRef = createRef<HTMLDivElement>();
  const owner = createDocumentScope<"sheet">();
  owner.set([{ slot: "sheet", host: options.host, content: createElement("div", {
    className: options.scrimClass, ref: rootRef,
    onMouseDown: (event: ReactMouseEvent) => { if (event.target === event.currentTarget) options.onScrim(); },
  }, createElement("div", { className: cls, role: "dialog", "aria-modal": true, "aria-label": label, ref: sheetRef })) }]);
  const root = rootRef.current, sheet = sheetRef.current;
  // The scope commits synchronously, so its DOM refs must be available here.
  if (!root || !sheet) { owner.dispose(); throw new Error("Dialog did not mount"); }
  return { root, sheet, dispose: owner.dispose };
}

/// A sheet on the page: a click on the scrim or Esc closes it, and focus goes
/// back to what had it. `close` always closes, so a caller guards its own buttons.
export function openSheet(label: string, cls: string, options: SheetOptions = {}) {
  const back = document.activeElement;
  const allowed = () => !options.canClose || options.canClose();
  const { root, sheet, dispose } = mountSheetFrame(label, cls, { host: options.host ?? document.body, scrimClass: "am-scrim", onScrim: () => { if (allowed()) close(); } });
  let open = true;
  const close = () => {
    if (!open) return;
    open = false;
    const index = openSheets.indexOf(root);
    if (index !== -1) openSheets.splice(index, 1);
    document.removeEventListener("keydown", keys, true);
    dispose();
    focusBack(back);
    options.onClose?.();
  };
  const keys = (e: KeyboardEvent) => {
    if (openSheets.at(-1) !== root) return;
    if (options.onKey?.(e) || e.key !== "Escape") return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (allowed()) close();
  };
  document.addEventListener("keydown", keys, true);
  openSheets.push(root);
  return { root, sheet, close };
}
