// Just enough of the webview for the bar's modules to load under Node:
// `#bar`, Tauri's globals and the observers some modules make at load.
// Import it before any `src/bar` module. `calls` records every `invoke`.

export const calls: { cmd: string; args: unknown }[] = [];

const g = globalThis as Record<string, unknown>;
const bar = { dataset: {}, innerHTML: "", querySelector: () => null, addEventListener() {} };
g.document = { getElementById: () => bar, querySelector: () => null, addEventListener() {}, createElement: () => ({ style: {} }) };
g.window = globalThis;
g.addEventListener = () => {};
g.ResizeObserver = class { observe() {} unobserve() {} };
g.MutationObserver = class { observe() {} };
g.__TAURI__ = {
  core: { invoke: async (cmd: string, args: unknown) => { calls.push({ cmd, args }); return null; }, Channel: class {} },
  event: { listen: async () => () => {} },
};
