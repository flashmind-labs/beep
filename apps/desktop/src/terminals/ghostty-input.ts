// What the user does to a Ghostty terminal, as the bytes its program reads:
// keys, mouse, paste and focus, each encoded by Ghostty against the
// terminal's current modes (application cursor keys, the Kitty keyboard
// protocol, mouse tracking, bracketed paste). And text selection, with
// Ghostty's own gestures: drag, double-click a word, triple-click a line.
//
// Nothing here goes through TerminalEffects.reply: input and the terminal's
// own answers stay apart.
import { Terminal, type CellSize, type Ghostty } from "./ghostty.ts";
import {
  GhosttyFocusEvent,
  GhosttyFormatterFormat,
  GhosttyFormatterOption,
  GhosttyGridRef,
  GhosttyKey,
  GhosttyKeyAction,
  GhosttyMouseAction,
  GhosttyMouseButton,
  GhosttyMouseEncoderOption,
  GhosttyMouseEncoderSize,
  GhosttyMousePosition,
  GhosttyPoint,
  GhosttyPointTag,
  GhosttyResult,
  GhosttySelection,
  GhosttySelectionGestureEventOption,
  GhosttySelectionGestureEventType,
  GhosttySelectionGestureGeometry,
  GhosttyString,
  GhosttySurfacePosition,
  GhosttyTerminalData,
  GhosttyTerminalModeConfig,
  GhosttyTerminalOption,
  GhosttyTerminalScreen,
} from "./ghostty-abi.ts";

/// Modifier bits of GhosttyMods (key/event.h).
const Mods = {
  SHIFT: 1 << 0,
  CTRL: 1 << 1,
  ALT: 1 << 2,
  SUPER: 1 << 3,
  SHIFT_SIDE: 1 << 6,
  CTRL_SIDE: 1 << 7,
  ALT_SIDE: 1 << 8,
  SUPER_SIDE: 1 << 9,
} as const;

/// DEC private modes the input reads.
const Mode = { FOCUS_EVENTS: 1004, BRACKETED_PASTE: 2004 } as const;

/// Clicks closer than this in time and space repeat: a double click picks a
/// word, a triple click a line. The browser does not say the system's own.
const REPEAT_INTERVAL_MS = 500;
const REPEAT_DISTANCE_PX = 6;

/// The most bytes one key, mouse or focus event encodes to.
const ENCODED_CAP = 128;

interface Modifiers {
  shift: boolean;
  ctrl: boolean;
  alt: boolean;
  /// Command on macOS, the Windows key elsewhere.
  meta: boolean;
}

/// A key as the page saw it: KeyboardEvent's layout-independent `code`, the
/// text it typed, and which way it went.
export interface KeyInput extends Modifiers {
  code: string;
  /// KeyboardEvent.key: the text typed, or a key name such as "Enter".
  key: string;
  action: "press" | "repeat" | "release";
}

export type MouseButton = "left" | "middle" | "right" | "wheel-up" | "wheel-down" | "wheel-left" | "wheel-right" | "none";

/// A pointer event over the grid, in CSS pixels from the grid's top left.
export interface MouseInput extends Modifiers {
  action: "press" | "release" | "motion";
  button: MouseButton;
  x: number;
  y: number;
}

/// Where a pointer is: its cell in the visible grid and its position.
export interface PointerAt {
  col: number;
  row: number;
  x: number;
  y: number;
}

/// Encodes input for one terminal. Free it with the terminal.
export class TerminalInput {
  private readonly g: Ghostty;
  private readonly term: Terminal;
  private readonly keyEncoder: number;
  private readonly keyEvent: number;
  private readonly mouseEncoder: number;
  private readonly mouseEvent: number;
  private readonly gesture: number;
  private readonly press: number;
  private readonly drag: number;
  private readonly release: number;
  /// Output buffers for encoded bytes and their length.
  private readonly buf: number;
  private readonly written: number;
  private grid = { cols: 0, rows: 0, cell: { width: 1, height: 1 } };

  constructor(g: Ghostty, term: Terminal) {
    this.g = g;
    this.term = term;
    const x = g.x;
    this.keyEncoder = this.made(out => x.ghostty_key_encoder_new(0, out), "ghostty_key_encoder_new");
    this.keyEvent = this.made(out => x.ghostty_key_event_new(0, out), "ghostty_key_event_new");
    this.mouseEncoder = this.made(out => x.ghostty_mouse_encoder_new(0, out), "ghostty_mouse_encoder_new");
    this.mouseEvent = this.made(out => x.ghostty_mouse_event_new(0, out), "ghostty_mouse_event_new");
    this.gesture = this.made(out => x.ghostty_selection_gesture_new(0, out), "ghostty_selection_gesture_new");
    const event = (type: GhosttySelectionGestureEventType) =>
      this.made(out => x.ghostty_selection_gesture_event_new(0, out, type), "ghostty_selection_gesture_event_new");
    this.press = event(GhosttySelectionGestureEventType.PRESS);
    this.drag = event(GhosttySelectionGestureEventType.DRAG);
    this.release = event(GhosttySelectionGestureEventType.RELEASE);

    const interval = g.out(8);
    g.view().setBigUint64(interval, BigInt(REPEAT_INTERVAL_MS * 1e6), true);
    this.set(this.press, GhosttySelectionGestureEventOption.REPEAT_INTERVAL_NS, interval);
    const distance = g.out(8);
    g.view().setFloat64(distance, REPEAT_DISTANCE_PX, true);
    this.set(this.press, GhosttySelectionGestureEventOption.REPEAT_DISTANCE, distance);
    this.buf = x.ghostty_wasm_alloc(ENCODED_CAP);
    this.written = x.ghostty_wasm_alloc(4);
  }

  /// Tells the encoders the grid's size, as the screen lays it out.
  resize(cols: number, rows: number, cell: CellSize) {
    this.grid = { cols, rows, cell };
    const out = this.g.out(GhosttyMouseEncoderSize.size);
    const view = this.g.view();
    const f = GhosttyMouseEncoderSize.field;
    view.setUint32(out + f.size, GhosttyMouseEncoderSize.size, true);
    view.setUint32(out + f.screen_width, cols * cell.width, true);
    view.setUint32(out + f.screen_height, rows * cell.height, true);
    view.setUint32(out + f.cell_width, cell.width, true);
    view.setUint32(out + f.cell_height, cell.height, true);
    check(this.g.x.ghostty_mouse_encoder_set(this.mouseEncoder, GhosttyMouseEncoderOption.SIZE, out), "setting the mouse grid");
  }

  /// The bytes for a key, or null when it sends nothing (a lone modifier,
  /// a release the program did not ask for).
  key(input: KeyInput): Uint8Array | null {
    const g = this.g;
    const x = g.x;
    check(x.ghostty_key_encoder_set_from_terminal(this.keyEncoder, this.term.handle), "reading the keyboard modes");

    const event = this.keyEvent;
    x.ghostty_key_event_set_action(event, keyAction(input.action));
    x.ghostty_key_event_set_key(event, keyOf(input.code));
    x.ghostty_key_event_set_mods(event, mods(input, input.code));
    x.ghostty_key_event_set_unshifted_codepoint(event, unshifted(input.code));

    const chars = typed(input);
    // Shift made the text (":" from ";"), so the kitty keyboard protocol
    // sends the text rather than Shift with the key, as Ghostty does.
    x.ghostty_key_event_set_consumed_mods(event, chars !== "" && input.shift ? Mods.SHIFT : 0);
    const text = new TextEncoder().encode(chars);
    const data = text.length > 0 ? x.ghostty_wasm_alloc(text.length) : 0;
    const str = x.ghostty_wasm_alloc(GhosttyString.size);

    try {
      if (data !== 0) g.bytes(data, text.length).set(text);
      const view = g.view();
      view.setUint32(str + GhosttyString.field.ptr, data, true);
      view.setUint32(str + GhosttyString.field.len, text.length, true);
      // Passed by value, which wasm32's C ABI passes as a pointer to a copy.
      check(x.ghostty_key_event_set_utf8(event, str), "setting the key's text");
      return this.encoded(x.ghostty_key_encoder_encode(this.keyEncoder, event, this.buf, ENCODED_CAP, this.written));
    } finally {
      x.ghostty_wasm_free(str, GhosttyString.size);
      if (data !== 0) x.ghostty_wasm_free(data, text.length);
    }
  }

  /// The bytes for a pointer event when the program tracks the mouse, or
  /// null when it does not, or the event says nothing new.
  mouse(input: MouseInput): Uint8Array | null {
    if (!this.mouseTracking()) return null;

    const g = this.g;
    const x = g.x;
    check(x.ghostty_mouse_encoder_set_from_terminal(this.mouseEncoder, this.term.handle), "reading the mouse modes");

    const event = this.mouseEvent;
    x.ghostty_mouse_event_set_action(event, mouseAction(input.action));
    const button = mouseButton(input.button);
    if (button === null) x.ghostty_mouse_event_clear_button(event);
    else x.ghostty_mouse_event_set_button(event, button);
    x.ghostty_mouse_event_set_mods(event, mods(input, ""));

    const pos = g.out(GhosttyMousePosition.size);
    const view = g.view();
    view.setFloat32(pos + GhosttyMousePosition.field.x, input.x, true);
    view.setFloat32(pos + GhosttyMousePosition.field.y, input.y, true);
    check(x.ghostty_mouse_event_set_position(event, pos), "setting the mouse position");

    return this.encoded(x.ghostty_mouse_encoder_encode(this.mouseEncoder, event, this.buf, ENCODED_CAP, this.written));
  }

  /// Pasted text as the program should read it: bracketed when it asked,
  /// with control characters that could end the paste made harmless.
  paste(text: string): Uint8Array {
    const g = this.g;
    const x = g.x;
    const bytes = new TextEncoder().encode(text);
    if (bytes.length === 0) return bytes;

    const bracketed = this.mode(Mode.BRACKETED_PASTE);
    const data = x.ghostty_wasm_alloc(bytes.length);
    // Room for the brackets around it.
    const cap = bytes.length + 16;
    const buf = x.ghostty_wasm_alloc(cap);

    try {
      g.bytes(data, bytes.length).set(bytes);
      check(x.ghostty_paste_encode(data, bytes.length, bracketed ? 1 : 0, buf, cap, this.written), "ghostty_paste_encode");
      return g.bytes(buf, g.view().getUint32(this.written, true)).slice();
    } finally {
      x.ghostty_wasm_free(buf, cap);
      x.ghostty_wasm_free(data, bytes.length);
    }
  }

  /// The bytes telling the program it gained or lost focus, when it asked.
  focus(gained: boolean): Uint8Array | null {
    if (!this.mode(Mode.FOCUS_EVENTS)) return null;

    const event = gained ? GhosttyFocusEvent.GAINED : GhosttyFocusEvent.LOST;
    return this.encoded(this.g.x.ghostty_focus_encode(event, this.buf, ENCODED_CAP, this.written));
  }

  /// Whether the program shows the alternate screen, as vim and less do.
  alternateScreen(): boolean {
    const out = this.g.out(4);
    check(this.g.x.ghostty_terminal_get(this.term.handle, GhosttyTerminalData.ACTIVE_SCREEN, out), "reading the screen");
    return this.g.view().getInt32(out, true) === GhosttyTerminalScreen.ALTERNATE;
  }

  /// Whether the program tracks the mouse, so clicks go to it rather than
  /// selecting text.
  mouseTracking(): boolean {
    const out = this.g.out(1);
    check(this.g.x.ghostty_terminal_get(this.term.handle, GhosttyTerminalData.MOUSE_TRACKING, out), "reading mouse tracking");
    return this.g.view().getUint8(out) !== 0;
  }

  /// A pointer pressed at `at`, `timeMs` from any steady clock: starts a
  /// selection, or picks a word or line when it repeats a click.
  selectPress(at: PointerAt, timeMs: number) {
    if (!this.ref(at, this.press)) return;
    this.position(this.press, at);
    const time = this.g.out(8);
    this.g.view().setBigUint64(time, BigInt(Math.round(timeMs * 1e6)), true);
    this.set(this.press, GhosttySelectionGestureEventOption.TIME_NS, time);
    this.apply(this.press);
  }

  selectDrag(at: PointerAt) {
    if (!this.ref(at, this.drag)) return;
    this.position(this.drag, at);

    const geometry = this.g.out(GhosttySelectionGestureGeometry.size);
    const view = this.g.view();
    const f = GhosttySelectionGestureGeometry.field;
    view.setUint32(geometry + f.columns, this.grid.cols, true);
    view.setUint32(geometry + f.cell_width, this.grid.cell.width, true);
    view.setUint32(geometry + f.padding_left, 0, true);
    view.setUint32(geometry + f.screen_height, this.grid.rows * this.grid.cell.height, true);
    this.set(this.drag, GhosttySelectionGestureEventOption.GEOMETRY, geometry);
    this.apply(this.drag);
  }

  selectRelease(at: PointerAt) {
    if (!this.ref(at, this.release)) this.set(this.release, GhosttySelectionGestureEventOption.REF, 0);
    this.apply(this.release);
  }

  clearSelection() {
    this.g.x.ghostty_selection_gesture_reset(this.gesture, this.term.handle);
    check(this.g.x.ghostty_terminal_set(this.term.handle, GhosttyTerminalOption.SELECTION, 0), "clearing the selection");
  }

  selectAll() {
    const out = this.g.out(GhosttySelection.size);
    if (this.g.x.ghostty_terminal_select_all(this.term.handle, out) !== GhosttyResult.SUCCESS) return;
    check(this.g.x.ghostty_terminal_set(this.term.handle, GhosttyTerminalOption.SELECTION, out), "selecting all");
  }

  /// The selected text, soft wraps joined, or null when nothing is selected.
  selectedText(): string | null {
    const g = this.g;
    const x = g.x;
    const selection = x.ghostty_wasm_alloc(GhosttySelection.size);
    let formatter = 0;

    try {
      if (x.ghostty_terminal_get(this.term.handle, GhosttyTerminalData.SELECTION, selection) !== GhosttyResult.SUCCESS) return null;

      formatter = this.made(out => x.ghostty_formatter_terminal_new(0, out, this.term.handle), "ghostty_formatter_terminal_new");
      const value = g.out(4);
      g.view().setInt32(value, GhosttyFormatterFormat.PLAIN, true);
      check(x.ghostty_formatter_set(formatter, GhosttyFormatterOption.EMIT, value), "setting the format");
      g.view().setUint8(value, 1);
      check(x.ghostty_formatter_set(formatter, GhosttyFormatterOption.UNWRAP, value), "setting unwrap");
      check(x.ghostty_formatter_set(formatter, GhosttyFormatterOption.TRIM, value), "setting trim");
      check(x.ghostty_formatter_set(formatter, GhosttyFormatterOption.SELECTION, selection), "setting the selection");

      const out = g.out(8);
      check(x.ghostty_formatter_format_alloc(formatter, 0, out, out + 4), "formatting the selection");
      const ptr = g.view().getUint32(out, true);
      const len = g.view().getUint32(out + 4, true);
      const text = new TextDecoder().decode(g.bytes(ptr, len));
      x.ghostty_free(0, ptr, len);
      return text;
    } finally {
      if (formatter !== 0) x.ghostty_formatter_free(formatter);
      x.ghostty_wasm_free(selection, GhosttySelection.size);
    }
  }

  free() {
    const x = this.g.x;
    x.ghostty_wasm_free(this.written, 4);
    x.ghostty_wasm_free(this.buf, ENCODED_CAP);
    x.ghostty_selection_gesture_event_free(this.release);
    x.ghostty_selection_gesture_event_free(this.drag);
    x.ghostty_selection_gesture_event_free(this.press);
    x.ghostty_selection_gesture_free(this.gesture, this.term.handle);
    x.ghostty_mouse_event_free(this.mouseEvent);
    x.ghostty_mouse_encoder_free(this.mouseEncoder);
    x.ghostty_key_event_free(this.keyEvent);
    x.ghostty_key_encoder_free(this.keyEncoder);
  }

  private mode(mode: number): boolean {
    const out = this.g.out(GhosttyTerminalModeConfig.size);
    this.g.view().setUint16(out + GhosttyTerminalModeConfig.field.mode, mode, true);
    if (this.g.x.ghostty_terminal_get(this.term.handle, GhosttyTerminalData.MODE, out) !== GhosttyResult.SUCCESS) return false;
    return this.g.view().getUint8(out + GhosttyTerminalModeConfig.field.value) !== 0;
  }

  /// Sets the gesture event's cell to the one at `at`; false when it is
  /// outside the grid.
  private ref(at: PointerAt, event: number): boolean {
    const g = this.g;
    const x = g.x;
    const point = x.ghostty_wasm_alloc(GhosttyPoint.size + GhosttyGridRef.size);
    const ref = point + GhosttyPoint.size;

    try {
      g.bytes(point, GhosttyPoint.size + GhosttyGridRef.size).fill(0);
      const view = g.view();
      view.setInt32(point + GhosttyPoint.field.tag, GhosttyPointTag.VIEWPORT, true);
      // The coordinate arm: x as u16, then y as u32.
      view.setUint16(point + GhosttyPoint.field.value, clamp(at.col, 0, this.grid.cols - 1), true);
      view.setUint32(point + GhosttyPoint.field.value + 4, clamp(at.row, 0, this.grid.rows - 1), true);
      if (x.ghostty_terminal_grid_ref(this.term.handle, point, ref) !== GhosttyResult.SUCCESS) return false;

      this.set(event, GhosttySelectionGestureEventOption.REF, ref);
      return true;
    } finally {
      x.ghostty_wasm_free(point, GhosttyPoint.size + GhosttyGridRef.size);
    }
  }

  private position(event: number, at: PointerAt) {
    const pos = this.g.out(GhosttySurfacePosition.size);
    const view = this.g.view();
    view.setFloat64(pos + GhosttySurfacePosition.field.x, at.x, true);
    view.setFloat64(pos + GhosttySurfacePosition.field.y, at.y, true);
    this.set(event, GhosttySelectionGestureEventOption.POSITION, pos);
  }

  private set(event: number, option: GhosttySelectionGestureEventOption, value: number) {
    check(this.g.x.ghostty_selection_gesture_event_set(event, option, value), "setting a selection gesture");
  }

  /// Runs a gesture event and shows the selection it makes, if any.
  private apply(event: number) {
    const x = this.g.x;
    const selection = x.ghostty_wasm_alloc(GhosttySelection.size);

    try {
      const result = x.ghostty_selection_gesture_event(this.gesture, this.term.handle, event, selection);
      if (result === GhosttyResult.SUCCESS) {
        check(x.ghostty_terminal_set(this.term.handle, GhosttyTerminalOption.SELECTION, selection), "showing the selection");
      } else if (result !== GhosttyResult.NO_VALUE) {
        check(result, "ghostty_selection_gesture_event");
      }
    } finally {
      x.ghostty_wasm_free(selection, GhosttySelection.size);
    }
  }

  private encoded(result: number): Uint8Array | null {
    if (result !== GhosttyResult.SUCCESS) return null;

    const len = this.g.view().getUint32(this.written, true);
    return len === 0 ? null : this.g.bytes(this.buf, len).slice();
  }

  /// Makes a Ghostty object through `make`, which writes its handle.
  private made(make: (out: number) => number, what: string): number {
    const out = this.g.x.ghostty_wasm_alloc(4);
    try {
      check(make(out), what);
      return this.g.view().getUint32(out, true);
    } finally {
      this.g.x.ghostty_wasm_free(out, 4);
    }
  }
}

/// The text a key types. Option is Alt, as it was with xterm.js
/// (macOptionIsMeta): Option+B types "b" with Alt, which the encoder sends as
/// ESC b, rather than the "∫" macOS makes of it. Ghostty's own option-as-alt
/// setting applies only to macOS builds, not this one. A key name such as
/// "Enter" types nothing.
function typed(input: KeyInput): string {
  if (input.alt && !input.ctrl && !input.meta) {
    const base = unshifted(input.code);
    if (base !== 0) {
      const char = String.fromCodePoint(base);
      return input.shift ? char.toUpperCase() : char;
    }
  }

  return [...input.key].length === 1 ? input.key : "";
}

/// GhosttyKey for a KeyboardEvent.code: the same names, in Ghostty's
/// spelling ("KeyA" is A, "ArrowLeft" is ARROW_LEFT, "Digit1" is DIGIT_1).
export function keyOf(code: string): GhosttyKey {
  const name = code
    .replace(/^Key(?=[A-Z]$)/, "")
    .replace(/([a-z])([A-Z0-9])/g, "$1_$2")
    .replace(/(Digit|Numpad)(\d)/g, "$1_$2")
    .toUpperCase();
  return name in GhosttyKey ? GhosttyKey[name as keyof typeof GhosttyKey] : GhosttyKey.UNIDENTIFIED;
}

/// The key's character without Shift on a US layout, which the Kitty
/// keyboard protocol reports; 0 for keys that type none.
export function unshifted(code: string): number {
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return letter[1].toLowerCase().codePointAt(0) ?? 0;

  const digit = /^Digit(\d)$/.exec(code);
  if (digit) return digit[1].codePointAt(0) ?? 0;

  const char = US_PUNCTUATION[code];
  return char === undefined ? 0 : (char.codePointAt(0) ?? 0);
}

const US_PUNCTUATION: Record<string, string> = {
  Backquote: "`",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Space: " ",
};

function mods(m: Modifiers, code: string): number {
  let bits = 0;
  if (m.shift) bits |= Mods.SHIFT | (code === "ShiftRight" ? Mods.SHIFT_SIDE : 0);
  if (m.ctrl) bits |= Mods.CTRL | (code === "ControlRight" ? Mods.CTRL_SIDE : 0);
  if (m.alt) bits |= Mods.ALT | (code === "AltRight" ? Mods.ALT_SIDE : 0);
  if (m.meta) bits |= Mods.SUPER | (code === "MetaRight" ? Mods.SUPER_SIDE : 0);
  return bits;
}

function keyAction(action: KeyInput["action"]): GhosttyKeyAction {
  switch (action) {
    case "press":
      return GhosttyKeyAction.PRESS;
    case "repeat":
      return GhosttyKeyAction.REPEAT;
    case "release":
      return GhosttyKeyAction.RELEASE;
  }
}

function mouseAction(action: MouseInput["action"]): GhosttyMouseAction {
  switch (action) {
    case "press":
      return GhosttyMouseAction.PRESS;
    case "release":
      return GhosttyMouseAction.RELEASE;
    case "motion":
      return GhosttyMouseAction.MOTION;
  }
}

/// Wheel directions are buttons four to seven, as X11 numbers them.
function mouseButton(button: MouseButton): GhosttyMouseButton | null {
  switch (button) {
    case "left":
      return GhosttyMouseButton.LEFT;
    case "middle":
      return GhosttyMouseButton.MIDDLE;
    case "right":
      return GhosttyMouseButton.RIGHT;
    case "wheel-up":
      return GhosttyMouseButton.FOUR;
    case "wheel-down":
      return GhosttyMouseButton.FIVE;
    case "wheel-left":
      return GhosttyMouseButton.SIX;
    case "wheel-right":
      return GhosttyMouseButton.SEVEN;
    case "none":
      return null;
  }
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, Math.floor(n)));
}

function check(result: number, what: string) {
  if (result !== GhosttyResult.SUCCESS) throw new Error(`${what} failed: ${result}`);
}
