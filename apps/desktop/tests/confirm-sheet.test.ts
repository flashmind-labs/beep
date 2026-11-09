import { JSDOM } from "jsdom";
import test from "node:test";
import assert from "node:assert/strict";

const dom = new JSDOM("<!doctype html><body></body>");
Object.assign(globalThis, {
  window: dom.window, document: dom.window.document,
  Node: dom.window.Node, HTMLElement: dom.window.HTMLElement,
});
const { flushSync } = await import("react-dom");
const { confirmSheet, renameSheet } = await import("../src/ui/menu.ts");
const settle = () => new Promise(resolve => setImmediate(resolve));

for (const dismiss of ["Escape", "Cancel"]) {
  test(`${dismiss} dismisses pending removal and completion keeps it closed`, async () => {
    let finish: (value: null) => void = () => {};
    const pending = new Promise<null>(resolve => { finish = resolve; });
    confirmSheet({ title: "Remove project?", text: "Stops syncing.", action: "Remove",
      dismissWhileBusy: true, run: () => pending });
    const action = document.querySelector<HTMLButtonElement>("button.warn");
    assert.ok(action);
    flushSync(() => action.click());
    assert.equal(action.disabled, true);

    if (dismiss === "Escape") {
      document.dispatchEvent(new dom.window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    } else {
      flushSync(() => document.querySelector<HTMLButtonElement>("button.quiet")?.click());
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
  flushSync(() => action.click());
  await settle();
  assert.equal(document.querySelector(".bad")?.textContent, "Removal failed");
  assert.equal(action.disabled, false);
  fail = false;
  flushSync(() => action.click());
  await settle();
  assert.equal(document.querySelector(".am-sheet"), null);
});

test("rename keeps its draft after failure and saves a retry", async () => {
  const { fireEvent } = await import("@testing-library/react");
  let fail = true;
  const saved: string[] = [];
  renameSheet({ title: "Rename item", value: "Original", run: async value => {
    saved.push(value);
    if (fail) throw new Error("Rename failed");
  } });
  const field = document.querySelector<HTMLInputElement>(".gp-field");
  assert.ok(field);
  assert.equal(document.activeElement, field);
  assert.equal(field.selectionStart, 0);
  assert.equal(field.selectionEnd, "Original".length);
  flushSync(() => fireEvent.change(field, { target: { value: "Changed" } }));
  const save = document.querySelector<HTMLButtonElement>("button.pri");
  assert.ok(save);
  flushSync(() => save.click());
  await settle();
  assert.equal(document.querySelector(".bad")?.textContent, "Rename failed");
  assert.equal(field.value, "Changed");
  fail = false;
  flushSync(() => save.click());
  await settle();
  assert.deepEqual(saved, ["Changed", "Changed"]);
  assert.equal(document.querySelector(".am-sheet"), null);
});

test("confirmation completion uses the current checkbox choice", async () => {
  let finish: (value: string) => void = () => {};
  const response = new Promise<string>(resolve => { finish = resolve; });
  confirmSheet({ title: "Remove item", text: "Remove it", action: "Remove", tick: { label: "Keep receipt", note: "Show receipt" }, run: () => response });
  const box = document.querySelector<HTMLInputElement>("input[type=checkbox]");
  const action = document.querySelector<HTMLButtonElement>("button.warn");
  assert.ok(box && action);
  flushSync(() => box.click());
  flushSync(() => action.click());
  flushSync(() => box.click());
  finish("Receipt");
  await settle();
  assert.equal(document.querySelector(".am-sheet"), null);
});
