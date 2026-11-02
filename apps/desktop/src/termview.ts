// A terminal screen drawn by xterm.js, for terminals.ts. Kept apart so the
// tests of terminals.ts load without a page.
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
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
  const fit = new FitAddon();
  term.loadAddon(fit);
  let opened = false;
  // Opened and fitted once the element is on the page with a size; output
  // written before then waits in the terminal's buffer.
  const fitNow = () => {
    if (!el.isConnected || !el.clientWidth || !el.clientHeight) return;
    if (!opened) { term.open(el); opened = true; }
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
    focus: () => term.focus(),
    onData: cb => { term.onData(cb); },
    onResize: cb => { term.onResize(cb); },
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
