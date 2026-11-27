// A terminal screen: Ghostty's engine (ghostty.ts) takes the program's
// output, the GPU renderer (ghostty-render.ts) draws it, and keys, mouse and
// paste are encoded by Ghostty (ghostty-input.ts). Kept apart from
// terminals.ts so its tests load without a page.
//
// Output is parsed in frames, at most a few milliseconds each, so a flood of
// output never stalls the window; the rest waits for the next frame.
import { DESKTOP_LIMITS, Ghostty, loadGhostty, PHONE_LIMITS, RenderState, Terminal, type CellSize } from "./ghostty.ts";
import { GridRenderer, type FontSpec, type Theme } from "./ghostty-render.ts";
import { TerminalInput, type KeyInput, type MouseButton } from "./ghostty-input.ts";
import { cellAt, fitGrid, routeKey, SYMBOLS_FONT, wheelAction, withSymbols, type Grid } from "./screen-state.ts";
import { makeTouchBar } from "./touchbar.tsx";
import { ZOOMED } from "../ui/zoom-state.ts";
import { APPLE, isTouchDevice, onPhone } from "../ui/platform.ts";
import { skinChanged } from "../ui/skin.ts";
import { logError } from "../ui/tauri.ts";
import type { Screen } from "./terminal-state.ts";

const FONT_SIZE = 12;
const LINE_HEIGHT = 1.2;
/// Time given to parsing output per frame.
const PARSE_BUDGET_MS = 6;
/// Output parsed between checks of the budget.
const PARSE_CHUNK = 64 * 1024;
const BLINK_MS = 530;
const encoder = new TextEncoder();
const decoder = new TextDecoder();

let engine: Promise<Ghostty> | null = null;

/// Ghostty's engine, compiled once for every screen of the window, once the
/// screen's fonts have loaded: a glyph drawn before is kept as drawn.
function ghostty(family: string): Promise<Ghostty> {
  engine ??= (async () => {
    const url = new URL("ghostty-vt.wasm", document.baseURI);
    const response = await fetch(url);
    if (!response.ok) throw new Error(`loading the terminal engine: ${response.status}`);
    const module = await WebAssembly.compile(await response.arrayBuffer());
    await loadFonts(family);
    return loadGhostty(module);
  })();
  return engine;
}

/// Loads the screen's text font and its symbols; a font that fails to load
/// leaves the browser's own fallback.
async function loadFonts(family: string) {
  const fonts = typeof document.fonts === "undefined" ? null : document.fonts;
  if (fonts === null) return;

  const loads = [fonts.load(`${FONT_SIZE}px ${family}`), fonts.load(`${FONT_SIZE}px ${SYMBOLS_FONT}`, "\u{f07b}")];
  for (const result of await Promise.allSettled(loads)) {
    if (result.status === "rejected") logError(`terminal font unavailable: ${result.reason}`);
  }
}

/// The engine's parts for one screen, made once the engine has loaded.
interface Engine {
  term: Terminal;
  state: RenderState;
  input: TerminalInput;
}

/// The canvas the renderer draws on.
function makeCanvas() {
  const canvas = document.createElement("canvas");
  canvas.className = "tm-canvas";
  return canvas;
}

/// A screen drawn in `el`, a new element unless one is given.
export function makeScreen(el: HTMLElement = document.createElement("div")): Screen {
  el.className = "tm-term";
  let canvas = makeCanvas();
  // Keys, IME and the soft keyboard type into this; it is never seen.
  const keys = document.createElement("textarea");
  keys.className = "tm-keys-input";
  keys.setAttribute("autocapitalize", "off");
  keys.setAttribute("autocomplete", "off");
  keys.setAttribute("autocorrect", "off");
  keys.spellcheck = false;
  keys.setAttribute("aria-label", "Terminal input");
  el.append(canvas, keys);

  const mono = getComputedStyle(document.documentElement).getPropertyValue("--mono").trim();
  const font: FontSpec = { family: withSymbols(mono || "'IBM Plex Mono', ui-monospace, monospace"), size: FONT_SIZE, lineHeight: LINE_HEIGHT };

  let parts: Engine | null = null;
  let renderer: GridRenderer | null = null;
  let gpuAttempted = false;
  /// The canvas held a context: a context dropped stays with its canvas, so
  /// the next renderer draws on a new one.
  let canvasUsed = false;
  let grid: Grid = { cols: 80, rows: 24 };
  let cell: CellSize = { width: 8, height: 16 };
  let dpr = 1;
  /// Output not yet parsed; `quiet` for output replayed from before the view
  /// attached, whose queries were asked of someone else long ago.
  const pending: { data: Uint8Array; quiet: boolean }[] = [];
  let quiet = false;
  let frame = 0;
  let parseTimer: ReturnType<typeof setTimeout> | undefined;
  let animation: ReturnType<typeof setTimeout> | undefined;
  let fullRedraw = true;
  let inputOn = false;
  let blinkOn = true;
  let selecting = false;
  let disposed = false;
  let theme = skinTheme();

  let onData: (data: string) => void = () => {};
  let onReply: (data: string) => void = () => {};
  let onResize: (size: Grid) => void = () => {};
  let escape: (() => void) | null = null;

  /// Sends what the user did to the program, when typing is on.
  const send = (bytes: Uint8Array | string | null) => {
    if (bytes === null || !inputOn) return;
    const data = typeof bytes === "string" ? bytes : decoder.decode(bytes);
    if (data === "") return;
    parts?.term.scrollToBottom();
    wake();
    onData(data);
  };

  // On a touch device: a key bar above the soft keyboard, and a screen that
  // shrinks to the visual viewport so the keyboard does not cover it.
  const bar = isTouchDevice() ? makeTouchBar(data => send(data)) : null;
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

  void ghostty(font.family).then(g => {
    if (disposed) return;
    const term = new Terminal(g, grid.cols, grid.rows, {
      reply: bytes => { if (inputOn && !quiet) onReply(decoder.decode(bytes)); },
      bell: () => {},
      titleChanged: () => {},
    }, onPhone() ? PHONE_LIMITS : DESKTOP_LIMITS);
    term.setColors(theme);
    parts = { term, state: new RenderState(g), input: new TerminalInput(g, term) };
    fitNow();
    schedule();
  }).catch(e => logError(`terminal engine unavailable: ${e instanceof Error ? e.stack ?? e.message : e}`));

  // A new skin: the screen takes its colours and draws again.
  const reskin = () => {
    theme = skinTheme();
    parts?.term.setColors(theme);
    if (renderer) renderer.theme = theme;
    fullRedraw = true;
    schedule();
  };
  skinChanged.add(reskin);

  /// Draws on the next animation frame, once. A hidden page has no frames,
  /// so there output is still parsed, on a timer: queries get their replies
  /// and what waits stays small.
  const schedule = () => {
    if (disposed) return;
    if (document.hidden) parseTimer ??= setTimeout(draw, 0);
    else if (frame === 0) frame = requestAnimationFrame(draw);
  };

  const draw = () => {
    cancelAnimationFrame(frame);
    clearTimeout(parseTimer);
    frame = 0;
    parseTimer = undefined;
    if (!parts || disposed) return;

    // Output first, within the frame's budget.
    const start = performance.now();
    while (pending.length > 0 && performance.now() - start < PARSE_BUDGET_MS) {
      const next = pending[0];
      quiet = next.quiet;
      if (next.data.length <= PARSE_CHUNK) {
        parts.term.write(next.data);
        pending.shift();
      } else {
        parts.term.write(next.data.subarray(0, PARSE_CHUNK));
        next.data = next.data.subarray(PARSE_CHUNK);
      }
      quiet = false;
    }
    if (pending.length > 0) schedule();

    clearTimeout(animation);
    const next = parts.term.tickAnimations(performance.now());
    if (next !== null) animation = setTimeout(schedule, Math.max(0, next - performance.now()));

    if (!renderer) return;
    if (renderer.lost()) {
      logError("terminal GPU context lost");
      dropGpu();
      return;
    }

    parts.state.update(parts.term);
    renderer.draw(parts.state, parts.term, fullRedraw, { blinkedOut: inputOn && !blinkOn, focused: inputOn && focused() });
    fullRedraw = false;
  };

  const dropGpu = () => {
    renderer?.dispose();
    renderer = null;
  };

  const focused = () => el.contains(document.activeElement);

  /// Fits the grid to the element, and makes the renderer once the screen
  /// is shown. Hidden screens keep their terminal but no GPU context.
  const fitNow = () => {
    layout();

    if (document.hidden || !el.isConnected || !el.clientWidth || !el.clientHeight) {
      dropGpu();
      gpuAttempted = false;
      return;
    }

    // Try once per shown period; a lost context is tried again when shown.
    if (!renderer && !gpuAttempted) {
      gpuAttempted = true;
      if (canvasUsed) {
        const fresh = makeCanvas();
        canvas.replaceWith(fresh);
        canvas = fresh;
      }
      canvasUsed = true;
      try {
        renderer = new GridRenderer(canvas, font, theme);
        fullRedraw = true;
      } catch (e) {
        logError(`terminal GPU rendering unavailable: ${e instanceof Error ? e.message : e}`);
      }
    }

    if (!renderer || !parts) return;

    dpr = window.devicePixelRatio || 1;
    cell = renderer.measure(dpr);
    const style = getComputedStyle(el);
    const width = el.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
    const height = el.clientHeight - parseFloat(style.paddingTop) - parseFloat(style.paddingBottom);
    const next = fitGrid(width, height, dpr, cell);
    const changed = next.cols !== grid.cols || next.rows !== grid.rows;
    grid = next;
    parts.term.resize(grid.cols, grid.rows, cell);
    parts.input.resize(grid.cols, grid.rows, cell);
    renderer.resize(grid.cols, grid.rows);
    fullRedraw = true;
    schedule();
    if (changed) onResize(grid);
  };

  /// A key as Ghostty's input takes it.
  const keyInput = (e: KeyboardEvent, action: KeyInput["action"]): KeyInput => ({
    code: e.code, key: e.key, action, shift: e.shiftKey, ctrl: e.ctrlKey, alt: e.altKey, meta: e.metaKey,
  });

  keys.addEventListener("keydown", e => {
    if (!parts || e.isComposing || e.keyCode === 229) return;

    const route = routeKey(e, APPLE, parts.input.alternateScreen(), escape !== null);
    switch (route.kind) {
      case "leave":
        e.preventDefault();
        escape?.();
        return;
      case "copy": {
        const text = parts.input.selectedText();
        if (text === null) return;
        e.preventDefault();
        void navigator.clipboard.writeText(text);
        return;
      }
      case "paste":
        // The browser's paste event follows, with the text.
        return;
      case "word":
        e.preventDefault();
        send(route.bytes);
        return;
      case "app":
        return;
      case "terminal": {
        const bytes = parts.input.key(keyInput(e, e.repeat ? "repeat" : "press"));
        if (bytes === null) return;
        e.preventDefault();
        parts.input.clearSelection();
        send(bar ? bar.apply(decoder.decode(bytes)) : bytes);
      }
    }
  });

  // Releases matter only to programs that asked for them (Kitty keyboard).
  keys.addEventListener("keyup", e => {
    if (!parts || e.isComposing || e.metaKey) return;
    send(parts.input.key(keyInput(e, "release")));
  });

  // Text the keydown did not send: IME, dead keys and soft keyboards.
  keys.addEventListener("compositionend", e => {
    send(e.data);
    keys.value = "";
  });
  keys.addEventListener("input", e => {
    if (!(e instanceof InputEvent) || e.isComposing) return;
    if (e.inputType === "insertText" && e.data) send(bar ? bar.apply(e.data) : e.data);
    else if (e.inputType === "deleteContentBackward") send("\x7f");
    else if (e.inputType === "insertLineBreak") send("\r");
    keys.value = "";
  });

  keys.addEventListener("paste", e => {
    e.preventDefault();
    const text = e.clipboardData?.getData("text/plain") ?? "";
    if (parts && text !== "") send(parts.input.paste(text));
  });
  keys.addEventListener("copy", e => {
    const text = parts?.input.selectedText() ?? null;
    if (text === null) return;
    e.preventDefault();
    e.clipboardData?.setData("text/plain", text);
  });

  keys.addEventListener("focus", () => { if (parts) send(parts.input.focus(true)); schedule(); });
  keys.addEventListener("blur", () => { if (parts) send(parts.input.focus(false)); schedule(); });

  /// Pointer events on the canvas, whichever canvas is current.
  function onCanvas<K extends "pointerdown" | "pointermove" | "pointerup" | "pointercancel" | "wheel">(
    type: K, handler: (e: HTMLElementEventMap[K]) => void, options?: AddEventListenerOptions,
  ) {
    el.addEventListener(type, e => { if (e.target === canvas) handler(e); }, options);
  }

  /// The pointer over the grid, from a pointer or wheel event.
  const at = (e: MouseEvent) => {
    const r = canvas.getBoundingClientRect();
    return cellAt(e.clientX - r.left, e.clientY - r.top, dpr, cell, grid);
  };
  const button = (e: MouseEvent): MouseButton => (e.button === 0 ? "left" : e.button === 1 ? "middle" : e.button === 2 ? "right" : "none");
  const mods = (e: MouseEvent) => ({ shift: e.shiftKey, ctrl: e.ctrlKey, alt: e.altKey, meta: e.metaKey });
  /// The program takes the mouse, unless Shift is held to select instead.
  const tracking = (e: MouseEvent) => !!parts && inputOn && !e.shiftKey && parts.input.mouseTracking();
  // A finger scrolls rather than selects.
  let touchY: number | null = null;

  onCanvas("pointerdown", e => {
    if (!parts) return;
    keys.focus({ preventScroll: true });

    if (e.pointerType === "touch") { touchY = e.clientY; return; }

    const p = at(e);
    canvas.setPointerCapture(e.pointerId);
    if (tracking(e)) {
      send(parts.input.mouse({ action: "press", button: button(e), x: p.x, y: p.y, ...mods(e) }));
      return;
    }

    if (e.button !== 0) return;
    selecting = true;
    parts.input.selectPress(p, e.timeStamp);
    fullRedraw = true;
    schedule();
    e.preventDefault();
  });

  onCanvas("pointermove", e => {
    if (!parts) return;

    if (touchY !== null) {
      const rows = Math.trunc(((touchY - e.clientY) * dpr) / cell.height);
      if (rows !== 0) {
        touchY -= (rows * cell.height) / dpr;
        parts.term.scrollBy(rows);
        fullRedraw = true;
        schedule();
      }
      return;
    }

    const p = at(e);
    if (selecting) {
      parts.input.selectDrag(p);
      fullRedraw = true;
      schedule();
    } else if (tracking(e)) {
      send(parts.input.mouse({ action: "motion", button: e.buttons & 1 ? "left" : e.buttons & 4 ? "middle" : e.buttons & 2 ? "right" : "none", x: p.x, y: p.y, ...mods(e) }));
    }
  });

  const pointerUp = (e: PointerEvent) => {
    if (!parts) return;
    if (touchY !== null) { touchY = null; return; }

    const p = at(e);
    if (selecting) {
      selecting = false;
      parts.input.selectRelease(p);
      fullRedraw = true;
      schedule();
    } else if (tracking(e)) {
      send(parts.input.mouse({ action: "release", button: button(e), x: p.x, y: p.y, ...mods(e) }));
    }
  };
  onCanvas("pointerup", pointerUp);
  onCanvas("pointercancel", pointerUp);

  onCanvas("wheel", e => {
    if (!parts) return;
    const action = wheelAction(e.deltaY, e.deltaMode === WheelEvent.DOM_DELTA_LINE, tracking(e), parts.input.alternateScreen());
    if (action === null) return;
    e.preventDefault();

    switch (action.kind) {
      case "mouse": {
        const p = at(e);
        for (let i = 0; i < action.times; i++) send(parts.input.mouse({ action: "press", button: action.button, x: p.x, y: p.y, ...mods(e) }));
        return;
      }
      case "keys":
        if (!inputOn) return;
        for (let i = 0; i < action.times; i++) {
          send(parts.input.key({ code: action.code, key: action.code, action: "press", shift: false, ctrl: false, alt: false, meta: false }));
        }
        return;
      case "scroll":
        parts.term.scrollBy(action.rows);
        fullRedraw = true;
        schedule();
    }
  }, { passive: false });

  // The cursor blinks on a timer that runs only while this terminal has the
  // keys in the focused window; otherwise it stays lit. A window losing focus,
  // hidden or not, takes focus from the terminal too.
  let blink: ReturnType<typeof setInterval> | null = null;
  const blinking = () => {
    const on = !disposed && inputOn && focused() && document.hasFocus();
    if (on && blink === null) blink = setInterval(() => { if (!blinking()) return; blinkOn = !blinkOn; schedule(); }, BLINK_MS);
    else if (!on && blink !== null) { clearInterval(blink); blink = null; }

    if (!on && !blinkOn) { blinkOn = true; schedule(); }
    return on;
  };
  // Focus leaves before the next element takes it.
  const focusChanged = () => { queueMicrotask(blinking); };
  el.addEventListener("focusin", focusChanged);
  el.addEventListener("focusout", focusChanged);

  /// Typing and output show the cursor and start its blink again, so it
  /// never blinks out while it moves.
  function wake() {
    if (blink !== null) { clearInterval(blink); blink = null; }
    if (!blinkOn) { blinkOn = true; schedule(); }
    blinking();
  }

  const resized = new ResizeObserver(fitNow);
  resized.observe(el);
  // A zoom changes the size of a cell, not always of the element.
  window.addEventListener(ZOOMED, fitNow);
  const visibility = () => { fitNow(); blinking(); schedule(); };
  document.addEventListener("visibilitychange", visibility);

  if (bar) {
    window.visualViewport?.addEventListener("resize", fitNow);
    window.visualViewport?.addEventListener("scroll", fitNow);
  }

  return {
    el,
    write: data => {
      pending.push({ data, quiet: false });
      wake();
      schedule();
    },
    replay: data => {
      pending.push({ data, quiet: true });
      schedule();
    },
    note: text => {
      pending.push({ data: encoder.encode(`\r\n\x1b[2m${text}\x1b[0m\r\n`), quiet: true });
      schedule();
    },
    size: () => grid,
    fit: fitNow,
    focus: () => keys.focus({ preventScroll: true }),
    focused,
    onData: cb => { onData = cb; },
    onReply: cb => { onReply = cb; },
    onResize: cb => { onResize = cb; },
    onEscape: cb => { escape = cb; },
    setInput: on => {
      inputOn = on;
      keys.readOnly = !on;
      blinkOn = true;
      blinking();
      layout();
      schedule();
    },
    dispose: () => {
      disposed = true;
      skinChanged.delete(reskin);
      cancelAnimationFrame(frame);
      clearTimeout(parseTimer);
      clearTimeout(animation);
      if (blink !== null) clearInterval(blink);
      resized.disconnect();
      window.removeEventListener(ZOOMED, fitNow);
      document.removeEventListener("visibilitychange", visibility);
      window.visualViewport?.removeEventListener("resize", fitNow);
      window.visualViewport?.removeEventListener("scroll", fitNow);
      bar?.dispose();
      dropGpu();
      if (parts) {
        parts.input.free();
        parts.state.free();
        parts.term.free();
        parts = null;
      }
    },
  };
}

/// The terminal's colours, from the skin's tokens: the pane's background and
/// text, the accent for the cursor and selection, and the status colours for
/// red, green, yellow, blue and bright black. On a light skin white turns grey,
/// so text a program writes in it stays readable.
function skinTheme(): Theme & Parameters<Terminal["setColors"]>[0] {
  return withSkinColors(color => {
    const background = color("--deep");
    const accent = color("--accent");
    const palette = new Map([[1, color("--err")], [2, color("--ok")], [3, color("--warn")], [4, color("--link")], [8, color("--muted")]]);
    if (luminance(background) > 0.5) {
      palette.set(7, color("--muted"));
      palette.set(15, color("--dim"));
    }

    return { background, foreground: color("--fg"), cursor: accent, selection: accent, selectionAlpha: 0.3, palette };
  });
}

/// Reads the skin's colour tokens as 0xRRGGBB: a probe resolves each to a
/// colour and a one-pixel canvas turns it into bytes, whatever CSS colour
/// syntax the skin writes it in. Black without a canvas.
function withSkinColors<T>(use: (color: (token: string) => number) => T): T {
  const probe = document.createElement("i");
  probe.hidden = true;
  document.documentElement.append(probe);
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = 1;
  const pen = canvas.getContext("2d", { willReadFrequently: true });

  const color = (token: string) => {
    if (!pen) return 0;
    probe.style.color = `var(${token})`;
    pen.clearRect(0, 0, 1, 1);
    pen.fillStyle = getComputedStyle(probe).color;
    pen.fillRect(0, 0, 1, 1);
    const [r, g, b] = pen.getImageData(0, 0, 1, 1).data;
    return (r << 16) | (g << 8) | b;
  };

  try {
    return use(color);
  } finally {
    probe.remove();
  }
}

/// Relative luminance of 0xRRGGBB, from 0 (black) to 1 (white), close enough
/// to tell a light background from a dark one.
function luminance(color: number) {
  return (0.2126 * ((color >> 16) & 0xff) + 0.7152 * ((color >> 8) & 0xff) + 0.0722 * (color & 0xff)) / 255;
}
