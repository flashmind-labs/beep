import { JSDOM } from "jsdom";
import { test } from "node:test";
import assert from "node:assert/strict";

const dom = new JSDOM("<body><nav></nav></body>");
Object.assign(globalThis, { window: dom.window, document: dom.window.document });
const { createElement } = await import("react");
const { flushSync } = await import("react-dom");
const { createRoot } = await import("react-dom/client");
const { PhonePaneTabs } = await import("../src/agentpane/tabs-view.tsx");

test("phone navigation retains buttons and focus as the selected pane changes", () => {
  const nav = document.querySelector("nav");
  assert.ok(nav);
  const root = createRoot(nav);
  const tabs = [{ id: "log", name: "Log" }, { id: "files", name: "Files" }];
  let selected = "log";
  const draw = () => flushSync(() => root.render(createElement(PhonePaneTabs, { tabs, selected, select: id => { selected = id; draw(); } })));
  draw();
  const files = nav.querySelector<HTMLButtonElement>('[data-mnav="files"]');
  assert.ok(files);
  files.focus();
  files.click();
  assert.equal(nav.querySelector('[data-mnav="files"]'), files);
  assert.equal(document.activeElement, files);
  assert.equal(files.getAttribute("aria-selected"), "true");
  assert.equal(nav.querySelector('[data-mnav="log"]')?.getAttribute("aria-selected"), "false");
  root.unmount();
});
