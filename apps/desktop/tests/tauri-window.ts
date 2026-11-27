import { JSDOM } from "jsdom";

// A window with Tauri's global API, for tests that load modules which read it at import.
const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
dom.window.matchMedia = query => ({ matches: false, media: query } as MediaQueryList);
const tauri = { core: { invoke: async () => null, Channel: class {} }, event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) } };
Object.defineProperty(dom.window, "__TAURI__", { value: tauri });
Object.assign(globalThis, { window: dom.window, document: dom.window.document, HTMLElement: dom.window.HTMLElement, Element: dom.window.Element, Node: dom.window.Node });
export const closeWindow = () => dom.window.close();
