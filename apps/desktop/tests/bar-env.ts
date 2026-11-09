// Just enough of the webview for the bar's modules to load under Node:
// `#bar`, Tauri's globals and the observers some modules make at load.
// Import it before any module of the bar's page. `calls` records every `invoke`;
// `answers` holds what a command returns (null when absent).

export const calls: { cmd: string; args: unknown }[] = [];
export const answers: Record<string, unknown> = {};

const g = globalThis as Record<string, unknown>;
export const bar = { dataset: {}, innerHTML: "", querySelector: () => null, querySelectorAll: () => [], addEventListener() {} };
g.document = { getElementById: () => bar, querySelector: () => null, addEventListener() {}, createElement: () => ({ style: {}, setAttribute() {} }) };
g.window = globalThis;
const windowListeners = new Map<string, Set<(event: Event) => void>>();
g.addEventListener = (name: string, receive: (event: Event) => void) => {
  let listeners = windowListeners.get(name);
  if (!listeners) windowListeners.set(name, listeners = new Set());
  listeners.add(receive);
};
g.dispatchEvent = (event: Event) => { for (const receive of windowListeners.get(event.type) ?? []) receive(event); return true; };
const nativeListeners = new Map<string, Set<(event: { payload: unknown }) => void>>();
export const nativeEvent = (name: string, payload?: unknown) => {
  for (const receive of nativeListeners.get(name) ?? []) receive({ payload });
};
g.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} };
g.MutationObserver = class { observe() {} };
g.HTMLTextAreaElement = class { get value() { return ""; } set value(_v: string) {} };
g.__TAURI__ = {
  core: { invoke: async (cmd: string, args: unknown) => { calls.push({ cmd, args }); return answers[cmd] ?? (cmd === "mesh_watch" ? new Promise(() => {}) : null); }, Channel: class {} },
  event: { listen: async (name: string, receive: (event: { payload: unknown }) => void) => {
    let listeners = nativeListeners.get(name);
    if (!listeners) nativeListeners.set(name, listeners = new Set());
    listeners.add(receive);
    return () => listeners.delete(receive);
  } },
};
