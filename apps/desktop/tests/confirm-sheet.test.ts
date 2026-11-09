import "./dom-env.ts";
import { JSDOM } from "jsdom";
import test from "node:test";
import assert from "node:assert/strict";
import { confirmSheet } from "../src/ui/menu.ts";

const dom = new JSDOM("<!doctype html><body></body>");
Object.assign(globalThis, {
  window: dom.window, document: dom.window.document,
  Node: dom.window.Node, HTMLElement: dom.window.HTMLElement,
});
const settle = () => new Promise(resolve => setImmediate(resolve));

for (const dismiss of ["Escape", "Cancel"]) {
  test(`${dismiss} dismisses pending removal and completion keeps it closed`, async () => {
    let finish: (value: null) => void = () => {};
    const pending = new Promise<null>(resolve => { finish = resolve; });
    confirmSheet({ title: "Remove project?", text: "Stops syncing.", action: "Remove",
      dismissWhileBusy: true, run: () => pending });
    const action = document.querySelector<HTMLButtonElement>("button.warn");
    assert.ok(action);
    action.click();
    assert.equal(action.disabled, true);

    if (dismiss === "Escape") {
      document.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    } else {
      document.querySelector<HTMLButtonElement>("button.quiet")?.click();
    }

    assert.equal(document.querySelector(".am-sheet"), null);
    finish(null);
    await settle();
    assert.equal(document.querySelector(".am-sheet"), null);
  });
}

test("successful removal closes automatically and failure permits retry", async () => {
  let fail = true;
  confirmSheet({ title: "Remove project?", text: "Stops syncing.", action: "Remove",
    dismissWhileBusy: true, run: async () => { if (fail) throw new Error("Removal failed"); return null; } });
  const action = document.querySelector<HTMLButtonElement>("button.warn");
  assert.ok(action);
  action.click();
  await settle();
  assert.equal(document.querySelector(".bad")?.textContent, "Removal failed");
  assert.equal(action.disabled, false);
  fail = false;
  action.click();
  await settle();
  assert.equal(document.querySelector(".am-sheet"), null);
});
