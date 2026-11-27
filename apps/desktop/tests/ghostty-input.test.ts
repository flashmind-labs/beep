import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { loadGhostty, Terminal, type Ghostty } from "../src/stores/terminals/ghostty.ts";
import { keyOf, TerminalInput, unshifted, type KeyInput, type MouseInput, type PointerAt } from "../src/stores/terminals/ghostty-input.ts";
import { GhosttyKey } from "../src/stores/terminals/ghostty-abi.ts";

const WASM = new URL("../ghostty/ghostty-vt.wasm", import.meta.url);
const CELL = { width: 8, height: 16 };
const engine: Promise<Ghostty> = WebAssembly.compile(readFileSync(WASM)).then(loadGhostty);

async function open(cols = 20, rows = 5) {
  const g = await engine;
  const term = new Terminal(g, cols, rows, { reply() {}, bell() {}, titleChanged() {} });
  term.resize(cols, rows, CELL);
  const input = new TerminalInput(g, term);
  input.resize(cols, rows, CELL);
  const write = (text: string) => term.write(new TextEncoder().encode(text));
  const done = () => { input.free(); term.free(); };
  return { term, input, write, done };
}

const NO_MODS = { shift: false, ctrl: false, alt: false, meta: false };

function key(code: string, k: string, extra: Partial<KeyInput> = {}): KeyInput {
  return { code, key: k, action: "press", ...NO_MODS, ...extra };
}

function text(bytes: Uint8Array | null): string | null {
  return bytes === null ? null : new TextDecoder().decode(bytes);
}

function cell(col: number, row: number): PointerAt {
  return { col, row, x: col * CELL.width + 1, y: row * CELL.height + 1 };
}

test("KeyboardEvent codes map to Ghostty keys", () => {
  assert.equal(keyOf("KeyA"), GhosttyKey.A);
  assert.equal(keyOf("ArrowLeft"), GhosttyKey.ARROW_LEFT);
  assert.equal(keyOf("Digit1"), GhosttyKey.DIGIT_1);
  assert.equal(keyOf("Numpad0"), GhosttyKey.NUMPAD_0);
  assert.equal(keyOf("NumpadEnter"), GhosttyKey.NUMPAD_ENTER);
  assert.equal(keyOf("F12"), GhosttyKey.F12);
  assert.equal(keyOf("BracketLeft"), GhosttyKey.BRACKET_LEFT);
  assert.equal(keyOf("ShiftRight"), GhosttyKey.SHIFT_RIGHT);
  assert.equal(keyOf("NoSuchKey"), GhosttyKey.UNIDENTIFIED);
  assert.equal(unshifted("KeyQ"), "q".codePointAt(0));
  assert.equal(unshifted("Slash"), "/".codePointAt(0));
  assert.equal(unshifted("Enter"), 0);
});

test("keys encode as a terminal sends them", async () => {
  const { input, done } = await open();
  assert.equal(text(input.key(key("KeyA", "a"))), "a");
  assert.equal(text(input.key(key("Enter", "Enter"))), "\r");
  assert.equal(text(input.key(key("KeyC", "c", { ctrl: true }))), "\x03");
  assert.equal(text(input.key(key("KeyB", "∫", { alt: true }))), "\x1bb");
  assert.equal(text(input.key(key("KeyB", "ı", { alt: true, shift: true }))), "\x1bB");
  assert.equal(text(input.key(key("ArrowUp", "ArrowUp"))), "\x1b[A");
  assert.equal(input.key(key("ShiftLeft", "Shift", { shift: true })), null);
  assert.equal(input.key(key("KeyA", "a", { action: "release" })), null);
  done();
});

test("keys follow the program's keyboard modes", async () => {
  const { input, write, done } = await open();
  // Application cursor keys, as vim and less ask for.
  write("\x1b[?1h");
  assert.equal(text(input.key(key("ArrowUp", "ArrowUp"))), "\x1bOA");
  // The Kitty keyboard protocol tells Escape apart.
  write("\x1b[>1u");
  assert.equal(text(input.key(key("Escape", "Escape"))), "\x1b[27u");
  // Text made with Shift stays text, as neovim's ":" needs; with every key
  // reported, Shift goes along with the key and its text.
  assert.equal(text(input.key(key("Semicolon", ":", { shift: true }))), ":");
  assert.equal(text(input.key(key("KeyA", "A", { shift: true }))), "A");
  write("\x1b[>31u");
  assert.equal(text(input.key(key("Semicolon", ":", { shift: true }))), "\x1b[59:58;2;58u");
  done();
});

test("the mouse goes to programs that track it", async () => {
  const { input, write, done } = await open();
  const press: MouseInput = { action: "press", button: "left", x: 2 * CELL.width + 1, y: CELL.height + 1, ...NO_MODS };
  assert.equal(input.mouseTracking(), false);
  assert.equal(input.mouse(press), null);

  write("\x1b[?1000h\x1b[?1006h");
  assert.equal(input.mouseTracking(), true);
  assert.equal(text(input.mouse(press)), "\x1b[<0;3;2M");
  assert.equal(text(input.mouse({ ...press, action: "release" })), "\x1b[<0;3;2m");
  assert.equal(text(input.mouse({ ...press, button: "wheel-up" })), "\x1b[<64;3;2M");
  done();
});

test("paste is bracketed when the program asks", async () => {
  const { input, write, done } = await open();
  assert.equal(text(input.paste("hi")), "hi");
  write("\x1b[?2004h");
  assert.equal(text(input.paste("hi")), "\x1b[200~hi\x1b[201~");
  done();
});

test("focus is reported only when the program asks", async () => {
  const { input, write, done } = await open();
  assert.equal(input.focus(true), null);
  write("\x1b[?1004h");
  assert.equal(text(input.focus(true)), "\x1b[I");
  assert.equal(text(input.focus(false)), "\x1b[O");
  done();
});

test("the alternate screen is seen", async () => {
  const { input, write, done } = await open();
  assert.equal(input.alternateScreen(), false);
  write("\x1b[?1049h");
  assert.equal(input.alternateScreen(), true);
  done();
});

test("dragging selects text, a double click a word", async () => {
  const { input, write, done } = await open();
  write("hello world\r\nsecond line");
  assert.equal(input.selectedText(), null);

  // A cell joins the selection once the pointer passes its middle.
  const end = { ...cell(4, 0), x: 5 * CELL.width - 1 };
  input.selectPress(cell(0, 0), 1000);
  input.selectDrag(end);
  input.selectRelease(end);
  assert.equal(input.selectedText(), "hello");

  input.clearSelection();
  assert.equal(input.selectedText(), null);

  input.selectPress(cell(7, 0), 5000);
  input.selectRelease(cell(7, 0));
  input.selectPress(cell(7, 0), 5100);
  input.selectRelease(cell(7, 0));
  assert.equal(input.selectedText(), "world");

  input.selectAll();
  assert.equal(input.selectedText(), "hello world\nsecond line");
  done();
});
