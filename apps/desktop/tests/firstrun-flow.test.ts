import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

test("first run remembers Start and closes the screen when another machine is linked", async () => {
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, localStorage: dom.window.localStorage, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  let machines = ["local"];
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async (cmd: string) => cmd === "list_machines" ? machines : null, Channel: class {} }, event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) } } });
  const { act, fireEvent, screen, cleanup } = await import("@testing-library/react");
  const { firstRun } = await import("../src/center/firstrun.ts");
  try {
    await act(async () => { await firstRun(); });
    const start = screen.getByRole("button", { name: /^Start/ });
    assert.equal(dom.window.document.activeElement, start);
    fireEvent.click(start);
    assert.equal(dom.window.localStorage.getItem("beep.firstRunDone"), "1");
    assert.equal(dom.window.document.querySelector(".fr-scrim"), null);
    // Linked, with the mesh still opening: the screen never shows.
    dom.window.localStorage.clear();
    machines = [];
    const linked = act(async () => { await firstRun(); });
    await new Promise(done => setTimeout(done, 150));
    assert.equal(dom.window.document.querySelector(".fr-scrim"), null);
    machines = ["local", "other"];
    await linked;
    assert.equal(dom.window.localStorage.getItem("beep.firstRunDone"), "1");
    assert.equal(dom.window.document.querySelector(".fr-scrim"), null);
  } finally {
    cleanup();
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
