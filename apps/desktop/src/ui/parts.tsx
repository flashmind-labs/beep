// Shared command-center controls, dialog lifetimes, focus and clipboard behavior.
import { createRef, useCallback, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactNode, type RefObject } from "react";
import { createDocumentScope } from "./document-root.tsx";

/// Gives focus back to what had it before a sheet opened.
export function focusBack(was: Element | null) {
  (was as HTMLElement | null)?.focus?.();
}

type SheetOptions = {
  /// Mount directly into the caller's host so React owns removal there.
  host?: HTMLElement;
  /// A class the scrim takes besides `am-scrim`.
  scrim?: string;
  /// False while the sheet must stay: a click on the scrim and the default Esc do nothing.
  canClose?: () => boolean;
  /// Sees every key first, ahead of the page; true when it took the key (Esc included).
  onKey?: (e: KeyboardEvent) => boolean | void;
  /// Runs once the sheet is gone.
  onClose?: () => void;
};

const openSheets: HTMLElement[] = [];

/// Whether a floating dialog was placed above this element.
export function dialogAbove(element: HTMLElement) {
  return [...dialogs].some(dialog => dialog.isConnected && !element.contains(dialog)
    && !!(element.compareDocumentPosition(dialog) & Node.DOCUMENT_POSITION_FOLLOWING));
}

/// The dialogs drawn now: sheets, menus that ask, the palette, popovers. Page
/// shortcuts stand aside while one is in the page.
const dialogs = new Set<Element>();
export const dialogOpen = () => [...dialogs].some(el => el.isConnected);

/// A dialog's ref: it counts as open while drawn. `own` also gets the element.
export function useDialogRef<T extends Element>(own: RefObject<T | null> | null = null) {
  return useCallback((el: T | null) => {
    if (own) own.current = el;
    if (!el) return;
    dialogs.add(el);
    return () => {
      dialogs.delete(el);
      if (own) own.current = null;
    };
  }, [own]);
}

interface SheetFrameOptions {
  host: HTMLElement; scrimClass: string; onScrim(): void;
  /// The dialog takes focus itself and keeps its keys from the page behind.
  ownKeys?: boolean;
}

/// A mounted React dialog frame. Its caller owns dismissal and focus behavior;
/// `render` puts the dialog's content in it, in the caller's flush, and may name it anew.
export function mountSheetFrame(label: string, cls: string, options: SheetFrameOptions) {
  const rootRef = createRef<HTMLDivElement>(), sheetRef = createRef<HTMLDivElement>();
  const owner = createDocumentScope<"sheet">();
  const frame = (content: ReactNode, named = label) => (
    <div className={options.scrimClass} ref={rootRef}
      onMouseDown={(event: ReactMouseEvent) => { if (event.target === event.currentTarget) options.onScrim(); }}>
      <div className={cls} role="dialog" aria-modal aria-label={named} ref={sheetRef}
        {...options.ownKeys ? { tabIndex: -1, onKeyDown: (event: ReactKeyboardEvent) => event.stopPropagation() } : {}}>{content}</div>
    </div>
  );
  owner.set([{ slot: "sheet", host: options.host, content: frame(null) }]);
  const root = rootRef.current, sheet = sheetRef.current;
  // The scope commits synchronously, so its DOM refs must be available here.
  if (!root || !sheet) { owner.dispose(); throw new Error("Dialog did not mount"); }
  dialogs.add(sheet);
  const render = (content: ReactNode, named = label) => owner.setDeferred([{ slot: "sheet", host: options.host, content: frame(content, named) }]);
  return { root, sheet, render, dispose: () => { dialogs.delete(sheet); owner.dispose(); } };
}

/// A sheet on the page: a click on the scrim or Esc closes it, and focus goes
/// back to what had it. `close` always closes, so a caller guards its own buttons.
export function openSheet(label: string, cls: string, options: SheetOptions = {}) {
  const back = document.activeElement;
  const allowed = () => !options.canClose || options.canClose();
  const { root, sheet, render, dispose } = mountSheetFrame(label, cls, { host: options.host ?? document.body, scrimClass: options.scrim ? `am-scrim ${options.scrim}` : "am-scrim", onScrim: () => { if (allowed()) close(); } });
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
    // Esc closes an open suggestion menu or popover first; it handles that.
    if (e.key === "Escape" && (document.activeElement?.getAttribute("aria-expanded") === "true" || dialogAbove(root))) return;
    if (options.onKey?.(e) || e.key !== "Escape") return;
    e.preventDefault();
    e.stopImmediatePropagation();
    if (allowed()) close();
  };
  document.addEventListener("keydown", keys, true);
  openSheets.push(root);
  return { root, sheet, close, render: (content: ReactNode, named = label) => { if (open) render(content, named); } };
}

const identities = new WeakMap<object, number>();
let nextIdentity = 0;

/// A number that stays with `object`, for a list key where the items have no id.
export function identity(object: object) {
  let id = identities.get(object);
  if (id === undefined) { id = ++nextIdentity; identities.set(object, id); }
  return id;
}
