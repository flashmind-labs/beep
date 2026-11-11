import { JSDOM } from "jsdom";

export const calls: { cmd: string; args: unknown }[] = [];
export const answers: Record<string, unknown> = {};
export const removed = new Set<object>();
const dom = new JSDOM("<!doctype html><div id='bar'></div>", { url: "https://app.test" });
const win = dom.window;
function chatRoot() {
  const root = win.document.getElementById("bar");
  if (!root) throw new Error("Missing chat test root");
  return root;
}
export const bar = chatRoot();
const frames = new Map<number, () => void>();
let frameId = 0;
export function runFrames() { const due = [...frames.values()]; frames.clear(); for (const frame of due) frame(); }
const raf = (callback: () => void) => { frames.set(++frameId, callback); return frameId; };
const cancel = (id: number) => { frames.delete(id); };
const tauri = { core: { invoke: async (cmd: string, args: unknown) => { calls.push({ cmd, args }); if (cmd === "mesh_watch" && !(cmd in answers)) return new Promise(() => {}); return answers[cmd] ?? null; }, Channel: class {} }, event: { listen: async () => () => {} } };
Object.defineProperty(win, "__TAURI__", { value: tauri });
// Libraries ask the window, not the global, for frames.
Object.assign(win, { requestAnimationFrame: raf, cancelAnimationFrame: cancel });
for (const [key, value] of Object.entries({ addEventListener: win.addEventListener.bind(win), removeEventListener: win.removeEventListener.bind(win), window: win, document: win.document, navigator: win.navigator, Node: win.Node, Element: win.Element, HTMLElement: win.HTMLElement, HTMLInputElement: win.HTMLInputElement, HTMLTextAreaElement: win.HTMLTextAreaElement, MutationObserver: win.MutationObserver, localStorage: win.localStorage, requestAnimationFrame: raf, cancelAnimationFrame: cancel, ResizeObserver: class { observe() {} unobserve() {} disconnect() {} } })) {
  Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
}
Object.defineProperty(win.HTMLElement.prototype, "innerText", { get() { return this.textContent ?? ""; }, set(value: string) { this.textContent = value; }, configurable: true });
const removeChild = win.Node.prototype.removeChild;
win.Node.prototype.removeChild = function <T extends Node>(child: T): T {
  if (child.isConnected) removed.add(child);
  return removeChild.call(this, child) as T;
};
