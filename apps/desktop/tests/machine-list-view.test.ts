import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import type { Machine } from "../src/screens/machines/machines.tsx";

test("machine list retains focused rows through live updates and routes actions", async () => {
  const dom = new JSDOM("<!doctype html><div id='bar' data-mode='window'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, localStorage: dom.window.localStorage, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async () => null, Channel: class {} }, event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) } } });
  const { render, fireEvent, cleanup } = await import("@testing-library/react");
  const { MachineList } = await import("../src/screens/machines/list-view.tsx");
  let selected = "", added = 0, linked = 0, remove = "", retried = "";
  const noOp = () => {};
  const machine: Machine = { id: "one", name: "build-box", online: true, can_host: true, seen_at: 0, version: "1.0", env: null };
  const props = { list: [machine], error: "", hosted: true, mac: null, newest: null, asking: null, adapterTries: new Map(), hints: null, age: () => "1m",
    open: (id: string) => { selected = id; }, add: () => { added++; }, link: () => { linked++; }, askRemove: (id: string) => { remove = id; }, cancelRemove: noOp, remove: noOp, retryAdapter: (id: string) => { retried = id; } };
  try {
    const view = render(createElement(MachineList, props));
    const row = view.getByRole("button", { name: "build-box" });
    row.focus();
    const list = view.container.querySelector<HTMLElement>(".dlist");
    assert.ok(list);
    list.scrollTop = 200;
    view.rerender(createElement(MachineList, { ...props, list: [{ ...machine, online: false }] }));
    assert.equal(view.getByRole("button", { name: "build-box" }), row);
    assert.equal(dom.window.document.activeElement, row);
    assert.equal(list.scrollTop, 200);
    fireEvent.keyDown(row, { key: "Enter" });
    assert.equal(selected, "one");
    fireEvent.click(view.getByRole("button", { name: "Add a machine" }));
    fireEvent.click(view.getByRole("button", { name: "Link another desktop" }));
    assert.equal(added, 1);
    assert.equal(linked, 1);
    const withAdapter: Machine = { ...machine, env: { os: "linux", os_version: "Linux", arch: "x86_64", hostname: "host", cpus: 2, memory_mb: 1024, home: "/path/to/work", tools: [], logins: [], avds: [], gpus: [], wsl: [] }, adapters: [{ agent: "codex", failed: "offline" }] };
    view.rerender(createElement(MachineList, { ...props, list: [withAdapter] }));
    selected = "";
    const retry = view.getByRole("button", { name: "Try again" });
    fireEvent.keyDown(retry, { key: "Enter" });
    fireEvent.keyDown(retry, { key: " " });
    assert.equal(selected, "", "keyboard events on a nested button do not open the machine");
    fireEvent.click(retry);
    assert.equal(retried, "one");
    assert.equal(selected, "", "retry does not open the machine");
    view.rerender(createElement(MachineList, { ...props, hosted: false }));
    fireEvent.click(view.getByRole("button", { name: "Remove build-box" }));
    assert.equal(remove, "one");
    view.rerender(createElement(MachineList, { ...props, hosted: false, asking: { id: "one", busy: true, error: "" } }));
    assert.equal((view.getByRole("button", { name: "Remove" }) as HTMLButtonElement).disabled, true);
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
