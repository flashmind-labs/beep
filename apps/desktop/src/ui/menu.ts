// Right-click menus in the window, and the sheet that asks before a step
// that cannot be undone, such as removing a machine.
import { focusBack, openSheet } from "./parts.ts";
import { button, el, present } from "./dom.ts";
import { reason } from "./format.ts";

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

/// Opens a menu titled `title` where `e` was clicked, in place of the system's.
export function openMenu(e: MouseEvent, title: string, items: readonly MenuItem[]) {
  e.preventDefault();
  e.stopPropagation();
  closeMenu();
  const back = document.activeElement;
  const box = el("div", "cc-menu", el("span", "lbl", title));
  box.setAttribute("role", "menu");
  box.setAttribute("aria-label", title);
  const close = () => {
    box.remove();
    window.removeEventListener("mousedown", away, true);
    window.removeEventListener("keydown", keys, true);
    window.removeEventListener("blur", close);
    window.removeEventListener("scroll", close, true);
    if (open?.box === box) open = null;
  };
  for (const item of items) {
    const b = button(item.label, item.warn ? "warn" : "", () => { close(); item.on(); });
    b.setAttribute("role", "menuitem");
    box.append(b);
  }
  const away = (ev: MouseEvent) => { if (!box.contains(ev.target as Node)) close(); };
  const keys = (ev: KeyboardEvent) => {
    if (ev.key === "Escape") { close(); focusBack(back); }
    else if (!arrowFocus(box, ev.key)) return;
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
  box.querySelector<HTMLElement>("button")?.focus();
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
  /// Does it; a failure shows in the sheet, which stays open to try again.
  run: () => Promise<unknown>;
}

/// Asks before `options.run`: Esc or Cancel closes the sheet, the action
/// button runs it and closes the sheet once it worked.
export function confirmSheet(options: ConfirmOptions) {
  closeMenu();
  let busy = false;
  const { sheet, close: shut } = openSheet(options.title, "am-sheet", {
    canClose: () => !busy,
    // A modal: no key reaches the page behind it.
    onKey: e => {
      if (e.key === "Escape") { e.preventDefault(); close(); }
      e.stopImmediatePropagation();
      return true;
    },
  });
  const error = el("div", "am-msg bad");
  error.hidden = true;
  const close = () => { if (!busy) shut(); };
  const go = async () => {
    if (busy) return;
    busy = true;
    act.disabled = true;
    error.hidden = true;
    try {
      await options.run();
      busy = false;
      close();
    } catch (e) {
      busy = false;
      act.disabled = false;
      error.textContent = reason(e);
      error.hidden = false;
    }
  };
  const act = button(options.action, "dbtn warn", go);
  sheet.append(...present([
    el("div", "am-head", el("b", "", options.title), el("kbd", "", "esc")),
    el("div", "am-msg", options.text),
    options.note ? el("div", "am-sub", options.note) : null,
    error,
    el("div", "gp-foot", button("Cancel", "dbtn quiet", close), act),
  ]));
  act.focus();
}
