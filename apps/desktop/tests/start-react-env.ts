import { JSDOM, type DOMWindow } from "jsdom";

export function startDom() {
  const dom = new JSDOM("<!doctype html><body><div id='bar'></div></body>", { url: "https://app.test" });
  Reflect.set(dom.window, "__TAURI__", Reflect.get(globalThis, "__TAURI__"));
  Object.assign(dom.window.HTMLElement.prototype, { attachEvent() {}, detachEvent() {} });
  // jsdom has no media queries: answer as a desktop with a mouse.
  Reflect.set(dom.window, "matchMedia", (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {} }));
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, HTMLElement: dom.window.HTMLElement, HTMLTextAreaElement: dom.window.HTMLTextAreaElement, HTMLInputElement: dom.window.HTMLInputElement, Element: dom.window.Element, Node: dom.window.Node, MutationObserver: dom.window.MutationObserver })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  return { dom, close() { dom.window.close(); for (const [key, descriptor] of previous) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key); } } };
}

export function routerWindowGlobals(view: DOMWindow) {
  return {
    self: view, history: view.history, localStorage: view.localStorage, sessionStorage: view.sessionStorage,
    addEventListener: view.addEventListener.bind(view), removeEventListener: view.removeEventListener.bind(view),
    Element: view.Element, Node: view.Node, HTMLButtonElement: view.HTMLButtonElement, SVGElement: view.SVGElement, HTMLTextAreaElement: view.HTMLTextAreaElement,
    requestAnimationFrame: (callback: FrameRequestCallback) => setTimeout(() => callback(performance.now()), 0),
    cancelAnimationFrame: (id: number) => clearTimeout(id),
  };
}
