import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement } from "react";
import { RunsView, type RunsViewProps } from "../src/parts/project/runs-view.tsx";
import type { RunRow, RunDetail } from "../src/parts/project/runs.tsx";

import type { PickButton } from "../src/parts/review/feedback-view.tsx";
const button: PickButton = { snapshot: () => ({ open: false, picker: null }), subscribe: () => () => {}, setBox() {}, setButton() {}, setSlot() {}, click() {}, why: () => "" };

test("run log updates retain the row, focus and scroll", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { createRoot } = await import("react-dom/client");
  const host = dom.window.document.getElementById("root"); assert.ok(host);
  const root = createRoot(host);
  const run: RunRow = { id: 1, number: 1, workflow: "Build", title: "Check", branch: "main", event: "push", actor: "Example", state: "running", url: "https://example.test/run", started_at: 1, finished_at: null, progress: null, host: "example.test" };
  const detail: RunDetail = { run, jobs: [], log: { job: "Test", step: null, text: "First line", truncated: false }, error: null, rate_limited_until: null };
  const props: RunsViewProps = { data: { runs: [run], checks: [], synced_at: null, error: null, rate_limited_until: null }, error: "", only: null, open: "example.test#1", mode: null, refresh() {}, filter() {}, toggle() {}, hover() {}, leave() {}, menu() {}, detail: () => ({ data: detail, error: "" }), fix: () => ({ button, state: { kind: "starting" }, error: "" }), openUrl() {} };
  try {
    await act(() => root.render(createElement(RunsView, props)));
    const row = host.querySelector<HTMLElement>(".rr-head"), log = host.querySelector<HTMLElement>(".pj-log pre"); assert.ok(row && log);
    row.focus(); log.scrollTop = 24;
    detail.log = { job: "Test", step: null, text: "First line\nNext line", truncated: false };
    await act(() => root.render(createElement(RunsView, { ...props })));
    assert.equal(host.querySelector(".rr-head"), row);
    assert.equal(host.querySelector(".pj-log pre"), log);
    assert.equal(dom.window.document.activeElement, row);
    assert.equal(log.scrollTop, 24);
    assert.equal(log.textContent, "First line\nNext line");
    await act(() => root.render(createElement(RunsView, { ...props, mode: { live: true, why: null } })));
    const mode = host.querySelector(".sw-mode"); assert.ok(mode);
    assert.equal(mode.textContent, "live");
    await act(() => root.render(createElement(RunsView, { ...props, mode: { live: false, why: "Webhook unavailable" } })));
    assert.equal(host.querySelector(".sw-mode"), mode);
    assert.equal(mode.textContent, "polling");
    assert.equal(mode.getAttribute("title"), "Polling: Webhook unavailable");
    assert.equal(dom.window.document.activeElement, row);
    assert.equal(log.scrollTop, 24);
    await act(() => root.render(createElement(RunsView, { ...props, mode: null })));
    assert.equal(host.querySelector(".sw-mode"), null);
  } finally {
    await act(() => root.unmount()); dom.window.close();
    for (const [key, descriptor] of previous) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key); }
  }
});
