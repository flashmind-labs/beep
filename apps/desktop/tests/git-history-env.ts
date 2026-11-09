import { JSDOM } from "jsdom";

export const calls: { cmd: string; args: unknown }[] = [];
const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "https://app.test" });
const view = dom.window;
Object.defineProperty(view.document, "visibilityState", { configurable: true, writable: true, value: "visible" });
Object.assign(globalThis, {
  window: view, document: view.document, HTMLElement: view.HTMLElement, HTMLDetailsElement: view.HTMLDetailsElement,
  HTMLButtonElement: view.HTMLButtonElement, Node: view.Node, Element: view.Element, SVGElement: view.SVGElement,
  requestAnimationFrame: (callback: FrameRequestCallback) => setTimeout(() => callback(performance.now()), 0),
  cancelAnimationFrame: (id: number) => clearTimeout(id),
  ResizeObserver: class { observe() {} unobserve() {} disconnect() {} },
});
Object.defineProperty(globalThis, "navigator", { configurable: true, value: view.navigator });
Object.defineProperty(view, "__TAURI__", { value: {
  core: { invoke: async (cmd: string, args: unknown) => { calls.push({ cmd, args }); if (cmd === "mesh_watch") return new Promise(() => {}); return null; }, Channel: class {} },
  event: { listen: async () => () => {} },
} });
