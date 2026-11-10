import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { ResearchHead } from "../src/research/head-view.tsx";
import type { Research } from "../src/research/research-state.ts";

test("research header preserves the running controls across streaming updates", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const host = dom.window.document.getElementById("root");
  assert.ok(host);
  const root = createRoot(host);
  const research: Research = { topic: "Example topic", status: "researching", agents: [], sources: [], draft: "", draft_html: "", started_at: 1, finished_at: null, round: 1 };
  try {
    await act(() => { root.render(createElement(ResearchHead, { research, entries: [], sending: "" })); });
    const stopButton = () => [...host.querySelectorAll("button")].find(button => button.textContent === "Stop") ?? null;
    const running = host.querySelector(".drun"), stop = stopButton();
    assert.ok(running && stop);
    stop.focus();
    await act(() => { root.render(createElement(ResearchHead, { research, entries: [], sending: "New streaming text" })); });
    assert.equal(host.querySelector(".drun"), running);
    assert.equal(stopButton(), stop);
    assert.equal(dom.window.document.activeElement, stop);
    await act(() => { root.render(createElement(ResearchHead, { research: { ...research, status: "done" }, entries: [], sending: null })); });
    assert.equal(stopButton(), null);
    assert.equal(host.querySelector(".chip")?.textContent, "Done");
  } finally {
    await act(() => { root.unmount(); });
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key);
    }
  }
});
