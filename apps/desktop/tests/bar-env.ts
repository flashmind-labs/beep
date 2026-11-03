// Just enough of the webview for the bar's modules to load under Node:
// `#bar`, Tauri's globals and the observers some modules make at load.
// Import it before any `src/bar` module. `calls` records every `invoke`;
// `answers` holds what a command returns (null when absent).

export const calls: { cmd: string; args: unknown }[] = [];
export const answers: Record<string, unknown> = {};

const g = globalThis as Record<string, unknown>;
export const bar = { dataset: {}, innerHTML: "", querySelector: () => null, addEventListener() {} };
g.document = { getElementById: () => bar, querySelector: () => null, addEventListener() {}, createElement: () => ({ style: {} }) };
g.window = globalThis;
g.addEventListener = () => {};
g.ResizeObserver = class { observe() {} unobserve() {} };
g.MutationObserver = class { observe() {} };
g.HTMLTextAreaElement = class { get value() { return ""; } set value(_v: string) {} };
g.__TAURI__ = {
  core: { invoke: async (cmd: string, args: unknown) => { calls.push({ cmd, args }); return answers[cmd] ?? null; }, Channel: class {} },
  event: { listen: async () => () => {} },
};
