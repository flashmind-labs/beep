// Right-click menus in the window, and the sheet that asks before a step
// that cannot be undone, such as removing a machine.
import { focusBack, openSheet } from "./parts.ts";
import { el } from "./dom.ts";
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createDocumentView, type DocumentView } from "./document-root.tsx";
import { ConfirmView, RenameView } from "./menu-views.tsx";

/// One line of a menu; `warn` reads it in red.
export interface MenuItem {
  label: string;
  warn?: boolean;
  on: () => void;
}

/// Moves focus to the next or previous button in `box` for ArrowDown or ArrowUp, round the
/// ends; false for any other key.
export function arrowFocus(box: HTMLElement, key: string) {
  if (key !== "ArrowDown" && key !== "ArrowUp") return false;
  const all = [...box.querySelectorAll<HTMLElement>("button")];
  const at = all.indexOf(document.activeElement as HTMLElement);
  all[(at + (key === "ArrowDown" ? 1 : all.length - 1)) % all.length]?.focus();
  return true;
}

let open: { box: HTMLElement; close: () => void } | null = null;

/// Opens a menu titled `title` where `e` was clicked, in place of the system's. The
/// returned function swaps in a fuller list while the menu is still open.
export function openMenu(e: MouseEvent, title: string, items: readonly MenuItem[]) {
  e.preventDefault();
  e.stopPropagation();
  closeMenu();
  const back = document.activeElement;
  const box = el("div", "cc-menu");
  const root = createDocumentView(box);
  let current = items;
  let MenuView: typeof import("./context-menu-view.tsx").ContextMenu | null = null;
  const close = () => {
    root.unmount();
    box.remove();
    window.removeEventListener("mousedown", away, true);
    window.removeEventListener("keydown", keys, true);
    window.removeEventListener("blur", close);
    window.removeEventListener("scroll", close, true);
    if (open?.box === box) open = null;
  };
  const fill = (list: readonly MenuItem[]) => {
    current = list;
    const View = MenuView;
    if (View) flushSync(() => root.render(createElement(View, { title, items: list, choose: (index: number) => { const item = current[index]; if (item) { close(); item.on(); } } })));
  };
  const away = (ev: MouseEvent) => { if (!box.contains(ev.target as Node)) close(); };
  const keys = (ev: KeyboardEvent) => {
    if (ev.key !== "Escape") return;
    close(); focusBack(back);
    ev.preventDefault();
    ev.stopImmediatePropagation();
  };
  document.body.append(box);
  // Kept inside the window.
  const r = box.getBoundingClientRect();
  box.style.left = `${Math.max(4, Math.min(e.clientX, innerWidth - r.width - 4))}px`;
  box.style.top = `${Math.max(4, Math.min(e.clientY, innerHeight - r.height - 4))}px`;
  window.addEventListener("mousedown", away, true);
  window.addEventListener("keydown", keys, true);
  window.addEventListener("blur", close);
  window.addEventListener("scroll", close, true);
  open = { box, close };
  void import("./context-menu-view.tsx").then(({ ContextMenu }) => {
    if (open?.box !== box) return;
    MenuView = ContextMenu;
    fill(current);
    const bounds = box.getBoundingClientRect();
    box.style.left = `${Math.max(4, Math.min(e.clientX, innerWidth - bounds.width - 4))}px`;
    box.style.top = `${Math.max(4, Math.min(e.clientY, innerHeight - bounds.height - 4))}px`;
    box.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
  });
  return (list: readonly MenuItem[]) => {
    if (open?.box !== box) return;
    const at = [...box.querySelectorAll('[role="menuitem"]')].indexOf(document.activeElement as HTMLElement);
    fill(list);
    box.querySelectorAll<HTMLElement>('[role="menuitem"]')[Math.max(at, 0)]?.focus();
    const r = box.getBoundingClientRect();
    box.style.top = `${Math.max(4, Math.min(e.clientY, innerHeight - r.height - 4))}px`;
  };
}

export function closeMenu() {
  open?.close();
}

/// What the confirm sheet says and does.
export interface ConfirmOptions {
  title: string;
  /// What happens, then what to know after.
  text: string;
  note?: string;
  /// The button that does it, such as "Remove".
  action: string;
  /// Allow dismissal while the operation continues in the background.
  dismissWhileBusy?: boolean;
  /// An unticked box under the text, with `note` in place of the sheet's
  /// note while it is ticked.
  tick?: { label: string; note: string };
  /// Does it, told whether the box is ticked; a failure shows in the sheet,
  /// which stays open to try again. With the box ticked, text it gives back
  /// stays shown in the sheet until it is closed.
  run: (ticked: boolean) => Promise<unknown>;
}

/// Asks before `options.run`: Esc or Cancel closes the sheet, the action
/// button runs it and closes the sheet once it worked.
export function confirmSheet(options: ConfirmOptions) {
  closeMenu();
  let busy = false, view: DocumentView | null = null;
  const allowed = () => !busy || options.dismissWhileBusy === true;
  const { sheet, close: shut } = openSheet(options.title, "am-sheet", {
    canClose: allowed,
    onClose: () => { view?.unmount(); view = null; },
    onKey: event => {
      if (event.key === "Escape") { event.preventDefault(); close(); }
      event.stopImmediatePropagation();
      return true;
    },
  });
  const close = () => { if (allowed()) shut(); };
  view = createDocumentView(sheet);
  flushSync(() => view?.render(createElement(ConfirmView, { options, close, pending: value => { busy = value; } })));
}

export interface RenameOptions { title: string; value: string; run(text: string): Promise<unknown> }

/// Saves a renamed value while keeping the draft available after a failure.
export function renameSheet(options: RenameOptions) {
  closeMenu();
  let busy = false, view: DocumentView | null = null;
  const { sheet, close: shut } = openSheet(options.title, "am-sheet", {
    canClose: () => !busy,
    onClose: () => { view?.unmount(); view = null; },
  });
  const close = () => { if (!busy) shut(); };
  view = createDocumentView(sheet);
  flushSync(() => view?.render(createElement(RenameView, { options, close, pending: value => { busy = value; } })));
}
