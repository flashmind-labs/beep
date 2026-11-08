import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";

let gpuLoads = 0;
let fits = 0;
class Terminal {
  attachCustomKeyEventHandler() {}
  loadAddon(addon: unknown) { if (addon instanceof WebglAddon) gpuLoads++; }
  open() {}
  dispose() {}
}
class FitAddon { fit() { fits++; } }
class WebglAddon { onContextLoss() {} }
Object.assign(globalThis, {
  terminalRendererTest: { Terminal, FitAddon, WebglAddon },
  document: { createElement: () => ({ isConnected: true, clientWidth: 800, clientHeight: 400 }) },
  getComputedStyle: () => ({ getPropertyValue: () => "monospace" }),
  window: { addEventListener() {}, removeEventListener() {} },
  ResizeObserver: class { observe() {} disconnect() {} },
});
const platformUrl = new URL("../src/ui/platform.ts", import.meta.url).href;
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
    if (url === platformUrl) return {
      format: "module", shortCircuit: true,
      source: "export let WINDOWS = false; export function setWindows(value) { WINDOWS = value; } export function commandKey(e) { return WINDOWS ? e.ctrlKey : e.metaKey; }",
    };
    const name = stubs.get(url.replace("renderer-test:", ""));
    return name ? {
      format: "module", shortCircuit: true,
      source: `export const ${name} = globalThis.terminalRendererTest.${name};`,
    } : next(url, context);
  },
});
const { makeScreen } = await import("../src/terminals/termview.ts");
const { setWindows } = await import(platformUrl);
hooks.deregister();

test("WebKit terminals fit without creating a GPU renderer; Windows keeps WebGL", () => {
  const mac = makeScreen();
  mac.fit();
  assert.equal(gpuLoads, 0);
  assert.equal(fits, 1);
  mac.dispose();

  setWindows(true);
  const windows = makeScreen();
  windows.fit();
  assert.equal(gpuLoads, 1);
  assert.equal(fits, 2);
  windows.dispose();
});
