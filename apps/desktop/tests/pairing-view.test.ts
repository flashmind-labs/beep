import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import type { PairingCommands } from "../src/screens/center/pairing-view.tsx";

test("pairing handles permissions, scan cancellation and a pasted retry", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { render, fireEvent, waitFor, cleanup } = await import("@testing-library/react");
  const { PairingScreen } = await import("../src/screens/center/pairing-view.tsx");
  let permission = "error", cancelled = false, paired = "", complete = 0, attempts = 0;
  let rejectScan: (error: Error) => void = () => {};
  const commands: PairingCommands = {
    checkPermission: async () => { if (permission === "error") throw new Error("cancelled"); return { camera: permission }; },
    requestPermission: async () => ({ camera: permission }),
    scan: () => {
      assert.ok(dom.window.document.querySelector(".pair.scanning"));
      return new Promise((_resolve, reject) => { rejectScan = reject; });
    },
    cancelScan: async () => { cancelled = true; rejectScan(new Error("cancelled")); },
    pair: async ticket => { paired = ticket; if (++attempts === 1) throw new Error("bad request: expired"); },
  };
  try {
    const view = render(createElement(PairingScreen, { commands, done: () => { complete++; } }));
    fireEvent.click(view.getByText("Scan QR code"));
    await waitFor(() => assert.equal(view.getByRole("alert").textContent, "cancelled"));
    permission = "denied";
    fireEvent.click(view.getByText("Scan QR code"));
    await waitFor(() => assert.match(view.getByRole("alert").textContent ?? "", /Camera access is off/));
    permission = "granted";
    fireEvent.click(view.getByText("Scan QR code"));
    await waitFor(() => assert.ok(view.getByText("Point the camera at the code")));
    fireEvent.click(view.getByText("Cancel"));
    await waitFor(() => assert.equal(view.getByRole("alert").textContent, "Scan cancelled."));
    assert.equal(cancelled, true);
    const field = view.getByPlaceholderText("Or paste the pairing code");
    fireEvent.change(field, { target: { value: "  example-ticket  " } });
    fireEvent.click(view.getByText("Pair"));
    await waitFor(() => assert.equal(view.getByRole("alert").textContent, "expired"));
    assert.equal(complete, 0);
    assert.equal((field as HTMLTextAreaElement).value, "  example-ticket  ");
    fireEvent.click(view.getByText("Pair"));
    await waitFor(() => assert.equal(complete, 1));
    assert.equal(paired, "example-ticket");
  } finally {
    cleanup();
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
