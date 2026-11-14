// Right-click menus in the window, and the sheet that asks before a step
// that cannot be undone, such as removing a machine.
import { focusBack, openSheet } from "./parts.tsx";
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createDocumentScope } from "./document-root.tsx";
import { ConfirmView, MenuBox, RenameView } from "./menu-views.tsx";

/// A line that does something; `warn` reads it in red. Untagged, as most
/// menus hold only these.
export interface MenuAction {
  kind?: "action";
  label: string;
  warn?: boolean;
  disabled?: boolean;
  on: () => void;
}

/// A line that switches a setting, with a check while `checked`.
export interface MenuToggle {
  kind: "toggle";
  label: string;
  checked: boolean;
  on: () => void;
}

/// Lines under a small label.
export interface MenuSection {
  kind: "section";
  label: string;
  items: readonly MenuLine[];
}

/// A rule between groups of lines.
export interface MenuSeparator { kind: "separator" }

export type MenuLine = MenuAction | MenuToggle;
export type MenuItem = MenuLine | MenuSection | MenuSeparator;

/// The lines of `items` in order, as the menu numbers them for choosing.
export function menuLines(items: readonly MenuItem[]): MenuLine[] {
  return items.flatMap(item => {
    switch (item.kind) {
      case "section": return item.items;
      case "separator": return [];
      default: return [item];
    }
  });
}

let open: { close: () => void } | null = null;

/// Opens a menu titled `title` where `e` was clicked, in place of the system's. The
/// returned function swaps in a fuller list while the menu is still open.
export function openMenu(e: MouseEvent, title: string, items: readonly MenuItem[]) {
  e.preventDefault();
  e.stopPropagation();
  closeMenu();
  const back = document.activeElement;
  const owner = createDocumentScope<"menu">();
  const at = { x: e.clientX, y: e.clientY };
  let current = items;
  let MenuView: typeof import("./context-menu-view.tsx").ContextMenu | null = null;
  const mine = { close: () => { owner.dispose(); if (open === mine) open = null; } };
  const choose = (index: number) => {
    const item = menuLines(current)[index];
    if (item) { mine.close(); item.on(); }
  };
  const escape = () => { mine.close(); focusBack(back); };
  const draw = () => {
    const View = MenuView;
    owner.set([{ slot: "menu", host: document.body, content: createElement(MenuBox, { at, close: mine.close, escape }, View ? createElement(View, { title, items: current, choose }) : null) }]);
  };

  draw();
  open = mine;
  void import("./context-menu-view.tsx").then(({ ContextMenu }) => {
    if (open !== mine) return;
    MenuView = ContextMenu;
    draw();
  });
  return (list: readonly MenuItem[]) => {
    if (open !== mine) return;
    current = list;
    draw();
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
  let busy = false;
  const allowed = () => !busy || options.dismissWhileBusy === true;
  const { close: shut, render } = openSheet(options.title, "am-sheet", {
    canClose: allowed,
    onKey: event => {
      if (event.key === "Escape") { event.preventDefault(); close(); }
      event.stopImmediatePropagation();
      return true;
    },
  });
  const close = () => { if (allowed()) shut(); };
  flushSync(() => render(createElement(ConfirmView, { options, close, pending: value => { busy = value; } })));
}

export interface RenameOptions { title: string; value: string; run(text: string): Promise<unknown> }

/// Saves a renamed value while keeping the draft available after a failure.
export function renameSheet(options: RenameOptions) {
  closeMenu();
  let busy = false;
  const { close: shut, render } = openSheet(options.title, "am-sheet", { canClose: () => !busy });
  const close = () => { if (!busy) shut(); };
  flushSync(() => render(createElement(RenameView, { options, close, pending: value => { busy = value; } })));
}
