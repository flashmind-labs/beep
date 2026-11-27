import { JSDOM } from "jsdom";
import { test } from "node:test";
import assert from "node:assert/strict";

const dom = new JSDOM("<body><input></body>");
Object.assign(globalThis, { window: dom.window, document: dom.window.document });
const { makeTouchBar } = await import("../src/parts/terminals/touchbar.tsx");

test("touch keys preserve terminal focus and apply modifiers once", () => {
  const sent: string[] = [];
  const bar = makeTouchBar(value => sent.push(value));
  bar.show(0);
  const keys = () => document.querySelector(".tm-keys");
  const terminal = document.querySelector("input");
  assert.ok(terminal);
  terminal.focus();
  const button = (name: string) => {
    const found = [...keys()?.querySelectorAll("button") ?? []].find(item => item.textContent === name);
    assert.ok(found);
    return found;
  };
  const ctrl = button("Ctrl");
  const pointer = new dom.window.Event("pointerdown", { bubbles: true, cancelable: true });
  assert.equal(ctrl.dispatchEvent(pointer), false);
  ctrl.click();
  assert.equal(document.activeElement, terminal);
  assert.equal(ctrl.getAttribute("aria-pressed"), "true");
  assert.equal(bar.apply("c"), "\x03");
  assert.equal(ctrl.getAttribute("aria-pressed"), "false");
  assert.equal(bar.apply("c"), "c");
  button("↑").click();
  button("↑").click();
  button("|").click();
  assert.deepEqual(sent, ["\x1b[A", "\x1b[A", "|"]);
  ctrl.click();
  assert.equal(bar.apply("paste"), "paste");
  bar.show(40);
  assert.equal(keys()?.getAttribute("style"), "bottom: 40px;");
  assert.equal(button("Ctrl"), ctrl);
  assert.equal(ctrl.getAttribute("aria-pressed"), "true");
  assert.equal(bar.apply("d"), "\x04");
  bar.show(null);
  assert.equal(keys()?.hasAttribute("hidden"), true);
  assert.equal(bar.height(), 0);
  bar.dispose();
  assert.equal(keys(), null);
});
