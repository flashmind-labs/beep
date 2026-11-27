import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { DiffToolbar } from "../src/parts/review/diff-toolbar.tsx";

test("diff toolbar preserves review controls and sends the current draft", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const host = dom.window.document.getElementById("root");
  assert.ok(host);
  let sent = 0, opened = 0, toggled = 0, files = 0;
  const feedback = createElement("button", { className: "dv-fb", onClick: () => { opened++; } });
  const props: Parameters<typeof DiffToolbar>[0] = {
    view: { root: "/srv/project", base: "main", merge_base: "", working_tree: false, additions: 0, deletions: 0, more: 0,
      files: [{ path: "sample.ts", old_path: null, status: "modified", additions: 0, deletions: 0, left_out: null, hunks: [] }] },
    loading: false, filter: "", found: 0, foundAt: -1, split: false, files: false, filesZoom: true, uncommitted: false, showUncommitted: true, allFolded: false,
    flash: "", comments: 2, sending: false, readOnly: false, feedback, findRef: null,
    onFilter() {}, onStep() {}, onFiles: () => { files++; }, onSplit() {}, onUncommitted: () => { toggled++; }, onFold() {}, onRefresh() {}, onSend: () => { sent++; },
  };
  const root = createRoot(host);
  try {
    await act(() => { root.render(createElement(DiffToolbar, props)); });
    const send = host.querySelector<HTMLButtonElement>("button.dbtn");
    const dirty = host.querySelector<HTMLButtonElement>('button[aria-label="Uncommitted only"]');
    assert.ok(send && dirty);
    assert.equal(send.hidden, false);
    assert.equal(send.title, "Send 2 comments to the agent as one message");
    const review = host.querySelector<HTMLButtonElement>("button.dv-fb");
    assert.ok(review);
    await act(() => { review.click(); dirty.click(); send.click(); });
    assert.equal(opened, 1);
    assert.equal(toggled, 1);
    assert.equal(sent, 1);
    const list = host.querySelector<HTMLButtonElement>("button.dv-filesbtn");
    assert.ok(list);
    assert.equal(list.title, "Show files full size (Shift+Esc)");
    await act(() => { list.click(); });
    assert.equal(files, 1);
    await act(() => { root.render(createElement(DiffToolbar, { ...props, sending: true, flash: "Sending" })); });
    assert.equal(host.querySelector("button.dbtn"), send);
    assert.equal(send.disabled, true);
    assert.equal(host.querySelector("button.dv-fb"), review);
    assert.equal(host.querySelector('[role="status"]')?.textContent, "Sending");
    await act(() => { root.render(createElement(DiffToolbar, { ...props, readOnly: true })); });
    assert.equal(send.hidden, true);
  } finally {
    await act(() => { root.unmount(); });
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key);
    }
  }
});
