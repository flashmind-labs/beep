import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement, type ComponentProps } from "react";
import { createRoot } from "react-dom/client";
import { ReportBody } from "../src/report/report-view.tsx";
import { START, type Draft, type ReportState } from "../src/report/report-state.ts";

test("report updates preserve the form and file list while exposing collection controls", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const host = dom.window.document.getElementById("root");
  assert.ok(host);
  const root = createRoot(host);
  let selected: string | null = null;
  const noOp = () => {};
  const props: ComponentProps<typeof ReportBody> = { state: START, title: "Report a problem", opened: null, issueText: null, debug: false,
    update: noOp, collect: noOp, send: noOp, back: noOp, retry: noOp, openIssue: noOp, cancel: noOp, done: noOp, startDebug: noOp,
    openFile: path => { selected = path; }, openUrl: noOp };
  const render = (state: ReportState, opened: string | null = null) => root.render(createElement(ReportBody, { ...props, state, opened }));

  try {
    await act(() => render(START));
    const textarea = host.querySelector("textarea");
    assert.ok(textarea);
    await act(() => render({ ...START, period: "hour" }));
    assert.equal(host.querySelector("textarea"), textarea);
    assert.equal(host.querySelector('[role="radio"][aria-checked="true"]')?.textContent, "Last hour");
    const draft: Draft = { manifest: { version: 1, sender: "example", mesh: null, app: "desktop", app_version: "0.1", text: "", period: "hour", since: 0, made_at: 0, machines: [], files: [{ path: "log.txt", bytes: 12 }] }, files: [{ path: "log.txt", text: "a [redacted] b" }] };
    const files: ReportState = { ...START, step: "files", draft };
    await act(() => render(files));
    const row = host.querySelector<HTMLButtonElement>("button.rp-row");
    assert.ok(row);
    await act(() => row.click());
    assert.equal(selected, "log.txt");
    await act(() => render(files, selected));
    assert.equal(host.querySelector("button.rp-row"), row);
    assert.equal(row.getAttribute("aria-expanded"), "true");
    assert.equal(host.querySelector(".rp-pane .r")?.textContent, "[redacted]");
    await act(() => render({ ...files, step: "sending" }, selected));
    assert.equal(host.querySelector<HTMLButtonElement>(".rp-foot .quiet")?.disabled, true);
    assert.equal(host.querySelector<HTMLButtonElement>(".rp-foot .pri")?.disabled, true);
  } finally {
    await act(() => root.unmount());
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
