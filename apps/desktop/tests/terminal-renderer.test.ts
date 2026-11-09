import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";

// termview.ts with the engine and the GPU renderer stubbed: what it does with
// the GPU as the screen is shown, hidden and loses its context.
let made = 0;
let disposed = 0;
let draws = 0;
let unavailable = false;
let lost = false;
const visibility = new Set<() => void>();

class GridRenderer {
  constructor() {
    made++;
    if (unavailable) throw new Error("WebGL2 is unavailable");
  }
  measure() { return { width: 8, height: 16 }; }
  resize() {}
  draw() { draws++; }
  lost() { return lost; }
  dispose() { disposed++; }
}
class Terminal {
  resize() {}
  write() {}
  setColors() {}
  scrollBy() {}
  scrollToBottom() {}
  tickAnimations() { return null; }
  free() {}
}
class RenderState { update() {} free() {} }
class TerminalInput { resize() {} free() {} alternateScreen() { return false; } mouseTracking() { return false; } }
const loadGhostty = async () => ({});

function element(): Record<string, unknown> {
  return {
    isConnected: true, clientWidth: 800, clientHeight: 400, style: {}, className: "",
    append() {}, setAttribute() {}, addEventListener() {}, contains: () => false,
    getBoundingClientRect: () => ({ left: 0, top: 0 }),
  };
}

const frames: (() => void)[] = [];
Object.assign(globalThis, {
  terminalRendererTest: { GridRenderer, Terminal, RenderState, TerminalInput, loadGhostty },
  document: {
    hidden: false, baseURI: "http://app/", activeElement: null,
    createElement: element,
    addEventListener: (_name: string, handler: () => void) => visibility.add(handler),
    removeEventListener: (_name: string, handler: () => void) => visibility.delete(handler),
    documentElement: {},
    body: { append() {} },
  },
  getComputedStyle: () => ({ getPropertyValue: () => "monospace", paddingLeft: "0", paddingRight: "0", paddingTop: "0", paddingBottom: "0" }),
  window: { addEventListener() {}, removeEventListener() {}, devicePixelRatio: 1 },
  ResizeObserver: class { observe() {} disconnect() {} },
  requestAnimationFrame: (f: () => void) => { frames.push(f); return frames.length; },
  cancelAnimationFrame() {},
  fetch: async () => ({ ok: true, arrayBuffer: async () => new ArrayBuffer(0) }),
});
WebAssembly.compile = async () => ({}) as WebAssembly.Module;

const stubs = new Map([
  ["./ghostty-render.ts", ["GridRenderer"]],
  ["./ghostty.ts", ["Terminal", "RenderState", "loadGhostty"]],
  ["./ghostty-input.ts", ["TerminalInput"]],
  ["./touchbar.ts", []],
]);
const hooks = registerHooks({
  resolve(specifier, context, next) {
    return stubs.has(specifier) ? { url: `renderer-test:${specifier}`, shortCircuit: true } : next(specifier, context);
  },
  load(url, context, next) {
    const names = stubs.get(url.replace("renderer-test:", ""));
    if (!names) return next(url, context);
    const source = url.endsWith("touchbar.ts")
      ? "export const isTouchDevice = () => false; export const makeTouchBar = () => null;"
      : names.map(n => `export const ${n} = globalThis.terminalRendererTest.${n};`).join("\n");
    return { format: "module", shortCircuit: true, source };
  },
});
const { makeScreen } = await import("../src/terminals/termview.ts");
hooks.deregister();

/// A screen whose engine has loaded.
async function screen() {
  const s = makeScreen();
  await new Promise(resolve => setTimeout(resolve, 0));
  return s;
}

function runFrames() {
  for (const f of frames.splice(0)) f();
}

test.beforeEach(() => {
  made = disposed = draws = 0;
  unavailable = lost = false;
  frames.length = 0;
  (globalThis as unknown as { document: { hidden: boolean } }).document.hidden = false;
});

test("a shown screen draws with the GPU, made once", async () => {
  const s = await screen();
  s.fit();
  s.fit();
  runFrames();
  assert.equal(made, 1);
  assert.ok(draws >= 1);
  s.dispose();
  assert.equal(disposed, 1);
  assert.equal(visibility.size, 0);
});

test("hidden screens release the GPU and get it back when shown", async () => {
  const s = await screen();
  s.fit();
  Object.assign(s.el, { clientWidth: 0 });
  s.fit();
  s.fit();
  assert.equal(disposed, 1);
  Object.assign(s.el, { clientWidth: 800 });
  s.fit();
  assert.equal(made, 2);
  s.dispose();
});

test("background windows release the GPU and get it back on return", async () => {
  const s = await screen();
  s.fit();
  const doc = (globalThis as unknown as { document: { hidden: boolean } }).document;
  doc.hidden = true;
  for (const handler of visibility) handler();
  assert.equal(disposed, 1);
  doc.hidden = false;
  for (const handler of visibility) handler();
  assert.equal(made, 2);
  s.dispose();
});

test("a lost context is dropped and not retried while shown", async () => {
  const s = await screen();
  s.fit();
  lost = true;
  s.write(new Uint8Array([0x61]));
  runFrames();
  assert.equal(disposed, 1);
  s.fit();
  assert.equal(made, 1);
  s.dispose();
});

test("a screen without WebGL2 stays usable", async t => {
  t.mock.method(console, "warn", () => {});
  unavailable = true;
  const s = await screen();
  s.fit();
  s.fit();
  s.write(new Uint8Array([0x61]));
  runFrames();
  assert.equal(made, 1);
  assert.deepEqual(s.size(), { cols: 80, rows: 24 });
  s.dispose();
});
