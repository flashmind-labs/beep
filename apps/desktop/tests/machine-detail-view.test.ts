import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement, type ComponentProps } from "react";
import type { Machine } from "../src/screens/machines/machines.tsx";

test("machine detail preserves a draft and log position on updates and routes its actions", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { render, fireEvent, cleanup } = await import("@testing-library/react");
  const { MachineDetail } = await import("../src/screens/machines/detail-view.tsx");
  const machine: Machine = { id: "one", name: "build-box", online: true, can_host: true, seen_at: 0, version: "1.0", env: { os: "linux", os_version: "Linux", arch: "x86_64", hostname: "host", cpus: 2, memory_mb: 1024, home: "/path/to/work", tools: [{ name: "codex", version: "1.0" }], logins: [{ tool: "codex", signed_in: false, account: null }], avds: [], gpus: [{ vendor: "NVIDIA", model: "NVIDIA GeForce RTX 4090", vram_mb: 24564, driver: "580.0" }], wsl: [] } };
  let saved = "", tool = "", node = 0, removed = 0, cancelled = 0, renamed = 0;
  const noOp = () => {};
  const props: ComponentProps<typeof MachineDetail> = { machine, invoke: () => Promise.reject(new Error("No native connection")), error: "", mac: null, newest: "2.0", updatable: true, nodeUpdate: null, search: { id: "one", folder: "/srv/project", draft: "/srv/project", busy: false, error: "" }, log: { id: "one", text: "line one\nline two", busy: false, error: "" }, tools: [{ machine_id: "one", tool: "codex", version: "1.0", latest: "2.0" }], toolUps: new Map(), installed: new Map(), traffic: undefined, asking: null, age: () => "1m", searchEdit: noOp, searchSave: value => { saved = value; }, searchCancel: () => { cancelled++; }, signIn: value => { tool = value; }, updateNode: () => { node++; }, updateTool: noOp, logRead: noOp, logSave: noOp, rename: () => { renamed++; }, askRemove: () => { removed++; }, remove: noOp, cancelRemove: noOp };
  try {
    const view = render(createElement(MachineDetail, props));
    view.getByText("NVIDIA GeForce RTX 4090 (24 GB)");
    fireEvent.click(view.getByText("Rename"));
    assert.equal(renamed, 1);
    const input = view.getByLabelText("Search for projects in") as HTMLInputElement;
    assert.equal(dom.window.document.activeElement, input);
    fireEvent.change(input, { target: { value: "/path/to/edited" } });
    const log = view.container.querySelector<HTMLElement>(".dlog");
    assert.ok(log);
    Object.defineProperty(log, "scrollHeight", { value: 1000 });
    Object.defineProperty(log, "clientHeight", { value: 100 });
    log.scrollTop = 7;
    fireEvent.scroll(log);
    view.rerender(createElement(MachineDetail, { ...props, error: "Connection issue" }));
    assert.equal(view.getByLabelText("Search for projects in"), input);
    assert.equal(input.value, "/path/to/edited");
    assert.equal(dom.window.document.activeElement, input);
    assert.equal(view.container.querySelector(".dlog"), log);
    assert.equal(log.scrollTop, 7);
    view.rerender(createElement(MachineDetail, { ...props, log: { id: "one", text: "line one\nline two\nline three", busy: false, error: "" } }));
    assert.equal(log.scrollTop, 7, "refresh keeps the reader position when not at the bottom");
    fireEvent.keyDown(input, { key: "Enter" });
    assert.equal(saved, "/path/to/edited");
    view.rerender(createElement(MachineDetail, { ...props, search: { id: "one", folder: "/srv/project", draft: "/srv/project", busy: true, error: "" } }));
    const cancel = view.getByRole("button", { name: "Cancel" }) as HTMLButtonElement;
    assert.equal(cancel.disabled, false);
    fireEvent.click(cancel);
    assert.equal(cancelled, 1);
    log.scrollTop = 900;
    fireEvent.scroll(log);
    view.rerender(createElement(MachineDetail, { ...props, log: { id: "one", text: "line one\nline two", busy: false, error: "offline" } }));
    assert.equal(view.container.querySelector(".dlog"), null);
    Object.defineProperty(dom.window.HTMLPreElement.prototype, "scrollHeight", { configurable: true, value: 1000 });
    view.rerender(createElement(MachineDetail, props));
    const restoredLog = view.container.querySelector<HTMLElement>(".dlog");
    assert.ok(restoredLog);
    assert.equal(restoredLog.scrollTop, 1000, "a log restored after an error follows the bottom");
    fireEvent.click(view.getByRole("button", { name: "Sign in" }));
    assert.equal(tool, "codex");
    fireEvent.click(view.getByRole("button", { name: "Update" }));
    assert.equal(node, 1);
    fireEvent.click(view.getByRole("button", { name: "Remove build-box" }));
    assert.equal(removed, 1);
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
