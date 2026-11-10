// The decisions of a terminal screen (termview.ts), apart from the page so
// they can be tested: how many cells fit, which cell a pointer is over, where
// a key goes and what the wheel does.
import { leavesTerminal } from "../agentpane/media-state.ts";
import { wordKeyBytes } from "./keys-state.ts";
import type { CellSize } from "./ghostty.ts";

/// Rows and columns kept at least, so a squeezed screen still works.
const MIN_COLS = 2;
const MIN_ROWS = 1;
/// Lines one wheel notch scrolls, and the pixels a notch is.
const WHEEL_LINES = 3;
const WHEEL_NOTCH_PX = 100;

export interface Grid {
  cols: number;
  rows: number;
}

/// The grid that fits `width` by `height` CSS pixels at `dpr`, in cells of
/// `cell` device pixels.
export function fitGrid(width: number, height: number, dpr: number, cell: CellSize): Grid {
  return {
    cols: Math.max(MIN_COLS, Math.floor((width * dpr) / cell.width)),
    rows: Math.max(MIN_ROWS, Math.floor((height * dpr) / cell.height)),
  };
}

/// The cell under a point `x`, `y` CSS pixels from the grid's top left,
/// with the point in device pixels, kept inside the grid.
export function cellAt(x: number, y: number, dpr: number, cell: CellSize, grid: Grid) {
  const px = Math.max(0, x * dpr);
  const py = Math.max(0, y * dpr);
  return {
    col: Math.min(grid.cols - 1, Math.floor(px / cell.width)),
    row: Math.min(grid.rows - 1, Math.floor(py / cell.height)),
    x: px,
    y: py,
  };
}

type KeyLike = Pick<KeyboardEvent, "key" | "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey" | "isComposing">;

/// Where a key typed in a screen goes:
/// - leave: Esc leaves a pane's terminal (not on the alternate screen)
/// - copy, paste: the clipboard (Cmd+C/V; Ctrl+Shift+C/V off macOS)
/// - word: a word or line key the shell reads as readline keys
/// - app: an app shortcut (Cmd on macOS), left to the window
/// - terminal: encoded for the program
export type KeyRoute =
  | { kind: "leave" }
  | { kind: "copy" }
  | { kind: "paste" }
  | { kind: "word"; bytes: string }
  | { kind: "app" }
  | { kind: "terminal" };

export function routeKey(e: KeyLike, mac: boolean, alternateScreen: boolean, canLeave: boolean): KeyRoute {
  if (canLeave && leavesTerminal(e, alternateScreen ? "alternate" : "normal")) return { kind: "leave" };

  const key = e.key.toLowerCase();
  const clipboard = mac ? e.metaKey && !e.ctrlKey && !e.altKey : e.ctrlKey && e.shiftKey && !e.altKey && !e.metaKey;
  if (clipboard && key === "c") return { kind: "copy" };
  if (clipboard && key === "v") return { kind: "paste" };

  const word = e.isComposing ? null : wordKeyBytes(e);
  if (word !== null) return { kind: "word", bytes: word };

  if (mac ? e.metaKey : e.metaKey && !e.ctrlKey) return { kind: "app" };
  return { kind: "terminal" };
}

/// What a wheel turn does:
/// - mouse: wheel buttons sent to a program that tracks the mouse
/// - keys: arrow keys for a full screen program that does not (less, man)
/// - scroll: the scrollback scrolls
export type WheelAction =
  | { kind: "mouse"; button: "wheel-up" | "wheel-down"; times: number }
  | { kind: "keys"; code: "ArrowUp" | "ArrowDown"; times: number }
  | { kind: "scroll"; rows: number };

/// `deltaY` in pixels, as WheelEvent with DOM_DELTA_PIXEL, or in lines.
export function wheelAction(deltaY: number, lines: boolean, mouseTracking: boolean, alternateScreen: boolean): WheelAction | null {
  const notches = lines ? deltaY / WHEEL_LINES : deltaY / WHEEL_NOTCH_PX;
  const rows = Math.trunc(notches * WHEEL_LINES) || Math.sign(deltaY);
  if (rows === 0) return null;

  const times = Math.abs(rows);
  if (mouseTracking) return { kind: "mouse", button: rows < 0 ? "wheel-up" : "wheel-down", times: Math.max(1, Math.round(times / WHEEL_LINES)) };
  if (alternateScreen) return { kind: "keys", code: rows < 0 ? "ArrowUp" : "ArrowDown", times };
  return { kind: "scroll", rows };
}

/// Icons and symbols that programs (yazi, prompts, editors) draw from Nerd
/// Fonts, bundled in fonts.css: no text font has them.
export const SYMBOLS_FONT = "'Symbols Nerd Font Mono'";

/// `family` with the symbols font before its generic fallbacks, which would
/// otherwise draw icons as missing glyphs.
export function withSymbols(family: string): string {
  const names = family.split(",").map(name => name.trim()).filter(name => name !== "");
  const generic = names.findIndex(name => ["monospace", "ui-monospace", "system-ui", "sans-serif", "serif"].includes(name));
  names.splice(generic === -1 ? names.length : generic, 0, SYMBOLS_FONT);
  return names.join(", ");
}
