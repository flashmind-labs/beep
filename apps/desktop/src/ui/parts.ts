// Small pieces of the command center window that several views draw the
// same way. They build on `el` and `button` from dom.ts.
import { button, el, type Kid } from "./dom.ts";
import type { Freshness } from "./pagecache.ts";

/// Fixed SVG markup as a node, never server text.
export function svgEl(markup: string) {
  const s = el("span");
  s.style.display = "inline-flex";
  s.innerHTML = markup;
  return s.firstChild;
}

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

/// A section: `secHead` over `body`, which the caller may give an id.
export function secBlock(title: Kid, sub: string, ...body: Kid[]) {
  return el("div", "", secHead(title, sub ? el("small", "", sub) : null), ...body);
}

/// A grid table with the column heads `cols`; `cls` adds to `cc-mt`.
export function headedTable(cls: string, ...cols: string[]) {
  return el("div", cls ? `cc-mt ${cls}` : "cc-mt", ...cols.map(t => el("span", "th", t)));
}

/// `rows` in a box as tall as its first `shown` rows; the rest scroll.
export function fitRows(rows: HTMLElement[], shown: number) {
  const list = el("div", "pj-scroll", ...rows);
  if (rows.length > shown && typeof requestAnimationFrame === "function") {
    requestAnimationFrame(() => {
      const tall = rows.slice(0, shown).reduce((sum, row) => sum + row.offsetHeight, 0);
      if (tall > 0) list.style.maxHeight = `${tall}px`;
    });
  }
  return list;
}

/// A note above a list after a dot: `dot` is "", "off", "warn" or "bad".
export function note(dot: string, ...kids: Kid[]) {
  return el("div", "cc-note", el("span", dot ? `ddot ${dot}` : "ddot"), el("span", "", ...kids));
}

/// A note in red above a list: what failed.
export function errorNote(text: string) {
  return el("div", "cc-note", el("span", "ddot bad"), el("span", "cc-err", text));
}

/// A section header's note on its data: a spinner while it reads again
/// data that is not new, else "updated 4 min ago"; null for nothing to say.
export function freshNote(f: Freshness) {
  if (f.spin) {
    const spin = el("span", "dspin sw-spin");
    spin.title = "Updating";
    return spin;
  }
  return f.text ? el("small", "sw-age", f.text) : null;
}

/// A state chip such as "running" or "needs reply"; `cls` colours it, and
/// `spin` puts a spinner before the text.
export function stateChip(cls: string, text: string, spin = false) {
  return el("span", cls ? `chip ${cls}` : "chip", spin ? el("span", "dspin") : null, text);
}

/// A segmented control: one button per option, the `current` one pressed.
export function segmented<T>(options: readonly { label: string; value: T }[], current: T, choose: (value: T) => void) {
  return el("span", "pb-seg", ...options.map(o => {
    const b = button(o.label, "", () => choose(o.value));
    b.setAttribute("aria-pressed", String(o.value === current));
    return b;
  }));
}

/// `text` on the clipboard. The webview may refuse the async way; the old way
/// still works.
export async function writeClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const t = el("textarea");
    t.value = text;
    document.body.append(t);
    t.select();
    document.execCommand("copy");
    t.remove();
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

/// A button with `cls` that copies `text`.
export function copyButton(text: string, label = "Copy", cls = "dbtn") {
  const b: HTMLButtonElement = button(label, cls, () => copy(text, b));
  return b;
}

/// A dialog over the window: the dimmed scrim holding the sheet, not yet
/// on the page.
export function sheetShell(label: string, cls: string) {
  const box = el("div", cls);
  box.setAttribute("role", "dialog");
  box.setAttribute("aria-modal", "true");
  box.setAttribute("aria-label", label);
  return { root: el("div", "am-scrim", box), sheet: box };
}

/// Gives focus back to what had it before a sheet opened.
export function focusBack(was: Element | null) {
  (was as HTMLElement | null)?.focus?.();
}

type SheetOptions = {
  /// False while the sheet must stay: a click on the scrim and the default Esc do nothing.
  canClose?: () => boolean;
  /// Sees every key first, ahead of the page; true when it took the key (Esc included).
  onKey?: (e: KeyboardEvent) => boolean | void;
  /// Runs once the sheet is gone.
  onClose?: () => void;
};

/// A sheet on the page: a click on the scrim or Esc closes it, and focus goes
/// back to what had it. `close` always closes, so a caller guards its own buttons.
export function openSheet(label: string, cls: string, options: SheetOptions = {}) {
  const back = document.activeElement;
  const { root, sheet } = sheetShell(label, cls);
  let open = true;
  const close = () => {
    if (!open) return;
    open = false;
    document.removeEventListener("keydown", keys, true);
    root.remove();
    focusBack(back);
    options.onClose?.();
  };
  const allowed = () => !options.canClose || options.canClose();
  const keys = (e: KeyboardEvent) => {
    if (options.onKey?.(e) || e.key !== "Escape") return;
    e.preventDefault();
    e.stopPropagation();
    if (allowed()) close();
  };
  root.addEventListener("mousedown", e => { if (e.target === root && allowed()) close(); });
  document.addEventListener("keydown", keys, true);
  document.body.append(root);
  return { root, sheet, close };
}
