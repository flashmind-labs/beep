import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

test("first run remembers Start and skips the screen when another machine is linked", async () => {
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, localStorage: dom.window.localStorage, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  let machines = ["local"], done = 0;
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async (cmd: string) => cmd === "list_machines" ? machines : null, Channel: class {} }, event: { listen: async () => () => {} } } });
  const { act, fireEvent, screen, cleanup } = await import("@testing-library/react");
  const { firstRun } = await import("../src/center/firstrun.ts");
  try {
    await act(async () => { await firstRun(false, () => { done++; }); });
    const start = screen.getByRole("button", { name: /^Start/ });
    assert.equal(dom.window.document.activeElement, start);
    fireEvent.click(start);
    assert.equal(done, 1);
    assert.equal(dom.window.localStorage.getItem("beep.firstRunDone"), "1");
    assert.equal(dom.window.document.querySelector(".fr-scrim"), null);
    dom.window.localStorage.clear();
    machines = ["local", "other"];
    await act(async () => { await firstRun(false, () => { done++; }); });
    assert.equal(done, 2);
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
