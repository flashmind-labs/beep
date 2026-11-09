import { JSDOM } from "jsdom";
import { test } from "node:test";
import assert from "node:assert/strict";

const dom = new JSDOM("<body><input></body>");
Object.assign(globalThis, { window: dom.window, document: dom.window.document });
const { makeTouchBar } = await import("../src/terminals/touchbar.ts");

test("touch keys preserve terminal focus and apply modifiers once", () => {
  const sent: string[] = [];
  const bar = makeTouchBar(value => sent.push(value));
  document.body.append(bar.el);
  bar.setShown(true);
  const terminal = document.querySelector("input");
  assert.ok(terminal);
  terminal.focus();
  const button = (name: string) => {
    const found = [...bar.el.querySelectorAll("button")].find(item => item.textContent === name);
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
  button("↑").click();
  button("↑").click();
  assert.deepEqual(sent, ["\x1b[A", "\x1b[A"]);
  ctrl.click();
  assert.equal(bar.apply("paste"), "paste");
  bar.el.remove();
  document.body.append(bar.el);
  assert.equal(button("Ctrl"), ctrl);
  assert.equal(ctrl.getAttribute("aria-pressed"), "true");
  assert.equal(bar.apply("d"), "\x04");
  bar.setShown(false);
  assert.equal(bar.height(), 0);
  bar.dispose();
  assert.equal(bar.el.isConnected, false);
});
