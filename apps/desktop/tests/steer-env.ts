import { JSDOM } from "jsdom";

export const calls: { cmd: string; args: unknown }[] = [];
export const answers: Record<string, unknown> = {};
const listeners = new Map<string, ((event: { payload: unknown }) => void)[]>();
/// Delivers an event to what the app listens with.
export const emit = (name: string, payload: unknown) => { for (const listener of listeners.get(name) ?? []) listener({ payload }); };
const dom = new JSDOM("<!doctype html><html><body><div id='bar'></div></body></html>", { url: "https://app.test" });
const view = dom.window;
Object.defineProperty(view.document, "hasFocus", { value: () => false });
Object.defineProperty(view, "addEventListener", { value() {} });
export const bar = view.document.getElementById("bar")!;
Object.assign(globalThis, {
  window: view, document: view.document, HTMLElement: view.HTMLElement, HTMLTextAreaElement: view.HTMLTextAreaElement,
  HTMLButtonElement: view.HTMLButtonElement, HTMLDetailsElement: view.HTMLDetailsElement,
  Node: view.Node, Element: view.Element, Event: view.Event, SVGElement: view.SVGElement, MutationObserver: view.MutationObserver,
  requestAnimationFrame: () => 1, cancelAnimationFrame() {}, addEventListener: view.addEventListener.bind(view),
  removeEventListener: view.removeEventListener.bind(view),
  ResizeObserver: class { observe() {} unobserve() {} disconnect() {} },
});
Object.defineProperty(globalThis, "navigator", { configurable: true, value: view.navigator });
Object.defineProperty(view, "__TAURI__", { value: {
  core: { invoke: async (cmd: string, args: unknown) => { calls.push({ cmd, args }); if (cmd === "mesh_watch" && !(cmd in answers)) return new Promise(() => {}); return answers[cmd] ?? null; }, Channel: class {} },
  event: { listen: async (name: string, listener: (event: { payload: unknown }) => void) => { listeners.set(name, [...(listeners.get(name) ?? []), listener]); return () => {}; } }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) },
} });
