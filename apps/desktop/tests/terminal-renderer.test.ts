import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";

let gpuLoads = 0;
let gpuDisposals = 0;
let fits = 0;
let redraws = 0;
let atlasClears = 0;
let unavailable = false;
let loseContext: () => void = () => {};
const visibility = new Set<() => void>();
const page = {
  hidden: false,
  createElement: () => ({ isConnected: true, clientWidth: 800, clientHeight: 400 }),
  addEventListener: (_name: string, handler: () => void) => visibility.add(handler),
  removeEventListener: (_name: string, handler: () => void) => visibility.delete(handler),
};
class Terminal {
  rows = 24;
  refresh(start: number, end: number) {
    assert.equal(start, 0);
    assert.equal(end, this.rows - 1);
    redraws++;
  }
  attachCustomKeyEventHandler() {}
  loadAddon(addon: unknown) {
    if (addon instanceof WebglAddon) {
      gpuLoads++;
      if (unavailable) throw new Error("GPU unavailable");
    }
  }
  open() {}
  dispose() {}
}
class FitAddon { fit() { fits++; } }
class WebglAddon {
  clearTextureAtlas() { atlasClears++; }
  onContextLoss(handler: () => void) { loseContext = handler; }
  dispose() { gpuDisposals++; }
}
Object.assign(globalThis, {
  terminalRendererTest: { Terminal, FitAddon, WebglAddon },
  document: page,
  getComputedStyle: () => ({ getPropertyValue: () => "monospace" }),
  window: { addEventListener() {}, removeEventListener() {} },
  ResizeObserver: class { observe() {} disconnect() {} },
});
const stubs = new Map([
  ["@xterm/xterm", "Terminal"],
  ["@xterm/addon-fit", "FitAddon"],
  ["@xterm/addon-webgl", "WebglAddon"],
]);
const hooks = registerHooks({
  resolve(specifier, context, next) {
    return stubs.has(specifier)
      ? { url: `renderer-test:${specifier}`, shortCircuit: true }
      : next(specifier, context);
  },
  load(url, context, next) {
    const name = stubs.get(url.replace("renderer-test:", ""));
    return name ? {
      format: "module", shortCircuit: true,
      source: `export const ${name} = globalThis.terminalRendererTest.${name};`,
    } : next(url, context);
  },
});
const { makeScreen } = await import("../src/terminals/termview.ts");
hooks.deregister();

test.beforeEach(() => {
  gpuLoads = gpuDisposals = fits = redraws = atlasClears = 0;
  unavailable = page.hidden = false;
});

test("visible terminals use GPU rendering without recreating it on resize", () => {
  const screen = makeScreen();
  screen.fit();
  screen.fit();
  assert.equal(gpuLoads, 1);
  assert.equal(fits, 2);
  Object.assign(screen.el, { isConnected: false });
  screen.fit();
  Object.assign(screen.el, { isConnected: true });
  screen.fit();
  assert.equal(gpuLoads, 2);
  assert.equal(redraws, 2);
  assert.equal(atlasClears, 2);
  screen.dispose();
  assert.equal(gpuDisposals, 2);
  assert.equal(visibility.size, 0);
});

test("hidden terminals release the GPU context and restore it when shown", () => {
  const screen = makeScreen();
  screen.fit();
  Object.assign(screen.el, { clientWidth: 0 });
  screen.fit();
  screen.fit();
  assert.equal(gpuDisposals, 1);
  Object.assign(screen.el, { clientWidth: 800 });
  screen.fit();
  assert.equal(gpuLoads, 2);
  screen.dispose();
});

test("background windows release GPU contexts and restore them on return", () => {
  const screen = makeScreen();
  screen.fit();
  page.hidden = true;
  for (const handler of visibility) handler();
  assert.equal(gpuDisposals, 1);
  page.hidden = false;
  for (const handler of visibility) handler();
  assert.equal(gpuLoads, 2);
  screen.dispose();
});

test("lost contexts fall back without repeated GPU retries while visible", () => {
  const screen = makeScreen();
  screen.fit();
  loseContext();
  screen.fit();
  assert.equal(gpuDisposals, 1);
  assert.equal(gpuLoads, 1);
  assert.equal(fits, 2);
  screen.dispose();
  assert.equal(gpuDisposals, 1);
});

test("unavailable GPU rendering leaves the terminal usable", t => {
  t.mock.method(console, "warn", () => {});
  unavailable = true;
  const screen = makeScreen();
  screen.fit();
  screen.fit();
  assert.equal(gpuLoads, 1);
  assert.equal(gpuDisposals, 1);
  assert.equal(fits, 2);
  screen.dispose();
});
