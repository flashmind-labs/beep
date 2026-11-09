import "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { RoutineEditor, RoutinesList } from "../src/lists/routines-view.tsx";
import type { Cron, RoutineForm } from "../src/lists/routines.ts";

test("routine list and editor retain their interactive elements across updates", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  dom.window.HTMLElement.prototype.scrollIntoView = () => {};
  Object.assign(dom.window.HTMLElement.prototype, { attachEvent() {}, detachEvent() {} });
  const host = dom.window.document.getElementById("root"); assert.ok(host);
  const root = createRoot(host);
  const routine: Cron = { id: "one", label: "Morning", prompt: "Summarise", schedule: "0 8 * * *", enabled: true, created_at: "", run_once: false, last_run_at: null, next_run_at: null };
  const list = (rows: Cron[]) => createElement(RoutinesList, { list: rows, selected: 0, mode: "list", loaded: true, error: "", summary: "1 on", grants: 0, hints: { items: [], window: false, model: null, link: null } });
  const form: RoutineForm = { edit: null, label: "", prompt: "", repeat: "day", days: [1], time: "08:00", date: "", cron: "", error: "", busy: false };
  const editor = () => createElement(RoutineEditor, { form, hints: { items: [], window: false, model: null, link: null }, input: (key, value) => { form[key] = value; } });
  try {
    await act(() => root.render(list([routine])));
    const row = host.querySelector('[role="option"]'), toggle = host.querySelector<HTMLButtonElement>('[role="switch"]');
    assert.ok(row && toggle); toggle.focus();
    await act(() => root.render(list([{ ...routine, prompt: "Updated" }])));
    assert.equal(host.querySelector('[role="option"]'), row); assert.equal(dom.window.document.activeElement, toggle);
    await act(() => root.render(editor()));
    const field = host.querySelector<HTMLInputElement>("#nrlabel"); assert.ok(field);
    field.focus(); field.value = "Draft"; field.setSelectionRange(2, 2);
    await act(() => { field.dispatchEvent(new dom.window.Event("input", { bubbles: true })); });
    form.repeat = "week";
    await act(() => root.render(editor()));
    assert.equal(host.querySelector("#nrlabel"), field); assert.equal(field.value, "Draft"); assert.equal(field.selectionStart, 2); assert.equal(dom.window.document.activeElement, field);
    assert.equal(host.querySelectorAll('[data-day]').length, 7);
  } finally {
    await act(() => root.unmount()); dom.window.close();
    for (const [key, descriptor] of previous) { if (descriptor) Object.defineProperty(globalThis, key, descriptor); else Reflect.deleteProperty(globalThis, key); }
  }
});
