import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

test("an unpaired phone mounts pairing before window startup and closes after pairing", async () => {
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  let paired = "";
  Object.defineProperty(dom.window, "__BEEP_PHONE__", { value: true });
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async (cmd: string, args: { ticket?: string }) => {
    if (cmd === "mobile_paired") return false;
    if (cmd === "mobile_pair") paired = args.ticket ?? "";
    return null;
  }, Channel: class {} }, event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) } } });
  const { act, fireEvent, screen, cleanup } = await import("@testing-library/react");
  const { pairing } = await import("../src/screens/center/pairing.tsx");
  try {
    let finished: Promise<void> = Promise.resolve();
    await act(async () => { finished = pairing(); await Promise.resolve(); });
    fireEvent.change(screen.getByPlaceholderText("Or paste the pairing code"), { target: { value: "example-ticket" } });
    await act(async () => { fireEvent.click(screen.getByText("Pair")); await finished; });
    assert.equal(paired, "example-ticket");
    assert.equal(dom.window.document.querySelector(".pair"), null);
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
