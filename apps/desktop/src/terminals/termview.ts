// A terminal screen drawn by xterm.js, for terminals.ts. Kept apart so the
// tests of terminals.ts load without a page.
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { outputBatch } from "./output-batch.ts";
import { leavesTerminal } from "../agentpane/media-state.ts";
import { wordKeyBytes } from "./keys-state.ts";
import { isTouchDevice, makeTouchBar } from "./touchbar.ts";
import { ZOOMED } from "../ui/zoom-state.ts";
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
    if (e.type === "keydown" && !e.isComposing) {
      const bytes = wordKeyBytes(e);

      if (bytes) {
        term.input(bytes, true);
        return false;
      }
    }

    if (e.type !== "keydown" || !escape || !leavesTerminal(e, term.buffer.active.type)) return true;
    escape();
    return false;
  });
  // On a touch device: a key bar above the soft keyboard, and a terminal that
  // shrinks to the visual viewport so the keyboard does not cover it.
  const bar = isTouchDevice() ? makeTouchBar(data => term.input(data, true)) : null;
  let inputOn = false;
  const layout = () => {
    if (!bar) return;
    const vv = window.visualViewport;
    const shown = inputOn && el.isConnected && !document.hidden && el.clientWidth > 0 && el.clientHeight > 0;
    if (!shown) { bar.show(null); el.style.maxHeight = ""; return; }
    const bottom = vv ? vv.height + vv.offsetTop : window.innerHeight;
    bar.show(Math.max(0, window.innerHeight - bottom));
    const room = bottom - el.getBoundingClientRect().top - bar.height();
    el.style.maxHeight = `${Math.max(120, Math.floor(room))}px`;
  };
  const output = outputBatch(data => term.write(data));
  const fit = new FitAddon();
  term.loadAddon(fit);
  let opened = false;
  let webgl: WebglAddon | undefined;
  let gpuAttempted = false;
  const dropGpu = () => {
    const addon = webgl;
    webgl = undefined;
    addon?.dispose();
  };
  // Asked for focus before it was opened: given once it is.
  let wantFocus = false;
  // Opened and fitted once the element is on the page with a size; output
  // written before then waits in the terminal's buffer.
  const fitNow = () => {
    layout();

    if (document.hidden || !el.isConnected || !el.clientWidth || !el.clientHeight) {
      // Hidden terminals keep their buffers, but need no GPU context.
      dropGpu();
      gpuAttempted = false;
      return;
    }

    if (!opened) {
      term.open(el);
      opened = true;
      if (wantFocus) { wantFocus = false; term.focus(); }
    }

    // Try once per visible period. A lost context falls back to the DOM
    // renderer until the terminal is hidden and shown again.
    if (!gpuAttempted) {
      gpuAttempted = true;

      try {
        webgl = new WebglAddon();
        webgl.onContextLoss(dropGpu);
        term.loadAddon(webgl);
        // Repaint kept output even when the terminal's size has not changed.
        webgl.clearTextureAtlas();
        term.refresh(0, term.rows - 1);
      } catch (e) {
        dropGpu();
        console.warn("Terminal GPU rendering unavailable:", e);
      }
    }

    fit.fit();
  };
  const resized = new ResizeObserver(fitNow);
  resized.observe(el);
  // A zoom changes the size of a cell, not always of the element.
  window.addEventListener(ZOOMED, fitNow);
  document.addEventListener("visibilitychange", fitNow);

  if (bar) {
    window.visualViewport?.addEventListener("resize", fitNow);
    window.visualViewport?.addEventListener("scroll", fitNow);
  }

  return {
    el,
    write: data => output.write(data),
    note: text => output.write(`\r\n\x1b[2m${text}\x1b[0m\r\n`),
    size: () => ({ cols: term.cols, rows: term.rows }),
    fit: fitNow,
    focus: () => { if (opened) term.focus(); else wantFocus = true; },
    focused: () => el.contains(document.activeElement),
    onData: cb => { term.onData(data => cb(bar ? bar.apply(data) : data)); },
    onResize: cb => { term.onResize(cb); },
    onEscape: cb => { escape = cb; },
    setInput: on => {
      term.options.disableStdin = !on;
      inputOn = on;
      layout();
      term.options.cursorBlink = on;
    },
    dispose: () => {
      output.dispose();
      resized.disconnect();
      window.removeEventListener(ZOOMED, fitNow);
      document.removeEventListener("visibilitychange", fitNow);
      window.visualViewport?.removeEventListener("resize", fitNow);
      window.visualViewport?.removeEventListener("scroll", fitNow);
      bar?.dispose();
      dropGpu();
      term.dispose();
    },
  };
}
