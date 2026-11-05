// A terminal screen drawn by xterm.js, for terminals.ts. Kept apart so the
// tests of terminals.ts load without a page.
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { leavesTerminal } from "../agentpane/media-state.ts";
import type { Screen } from "./terminals.ts";

/// The terminal's colours, as the mock's terminal pane.
const THEME = {
  background: "#0B0F0D",
  foreground: "#CFE3D8",
  cursor: "#6FCF97",
  selectionBackground: "rgba(111,207,151,.3)",
  green: "#6FCF97",
  brightBlack: "#6B7D73",
  yellow: "#E9B949",
};

export function makeScreen(): Screen {
  const el = document.createElement("div");
  el.className = "tm-xterm";
  const mono = getComputedStyle(document.documentElement).getPropertyValue("--mono").trim();
  const term = new Terminal({
    fontFamily: mono || "'IBM Plex Mono', ui-monospace, monospace",
    fontSize: 12,
    lineHeight: 1.2,
    cursorBlink: true,
    scrollback: 5000,
    macOptionIsMeta: true,
    disableStdin: true,
    theme: THEME,
  });
  // Set by a pane's terminal: Esc leaves it instead of reaching the shell.
  let escape: (() => void) | null = null;
  term.attachCustomKeyEventHandler(e => {
    if (e.type !== "keydown" || !escape || !leavesTerminal(e, term.buffer.active.type)) return true;
    escape();
    return false;
  });
  const fit = new FitAddon();
  term.loadAddon(fit);
  let opened = false;
  // Asked for focus before it was opened: given once it is.
  let wantFocus = false;
  // Opened and fitted once the element is on the page with a size; output
  // written before then waits in the terminal's buffer.
  const fitNow = () => {
    if (!el.isConnected || !el.clientWidth || !el.clientHeight) return;
    if (!opened) {
      term.open(el);
      opened = true;
      if (wantFocus) { wantFocus = false; term.focus(); }
    }
    fit.fit();
  };
  const resized = new ResizeObserver(fitNow);
  resized.observe(el);
  return {
    el,
    write: data => term.write(data),
    note: text => term.write(`\r\n\x1b[2m${text}\x1b[0m\r\n`),
    size: () => ({ cols: term.cols, rows: term.rows }),
    fit: fitNow,
    focus: () => { if (opened) term.focus(); else wantFocus = true; },
    focused: () => el.contains(document.activeElement),
    onData: cb => { term.onData(cb); },
    onResize: cb => { term.onResize(cb); },
    onEscape: cb => { escape = cb; },
    setInput: on => {
      term.options.disableStdin = !on;
      term.options.cursorBlink = on;
    },
    dispose: () => {
      resized.disconnect();
      term.dispose();
    },
  };
}
