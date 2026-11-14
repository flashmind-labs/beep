import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import type { MachineView } from "../src/machines/machine-state.ts";

test("adding a WSL distribution shows its steps, then that it was added", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { WslRows } = await import("../src/machines/wsl-view.tsx");
  const { render, fireEvent, findByText, cleanup } = await import("@testing-library/react");
  const host: MachineView = { id: "desk", name: "desk-pc", online: true, seen_at: 0, version: "1.0", env: { os: "windows", os_version: "Windows", arch: "x86_64", hostname: "desk-pc", cpus: 2, memory_mb: 1024, home: "/path/to/work", tools: [], logins: [], avds: [], wsl: [{ name: "Ubuntu", agents: ["claude"], node: null, machine_id: null, kept: false, problem: null }] } };
  let finish = () => {};
  const asked: string[] = [];
  const actions = {
    add: (machine: string, distro: string, step: (text: string) => void) => new Promise<void>(resolve => { asked.push(`${machine} ${distro}`); step("Installing beep-node in Ubuntu"); finish = resolve; }),
    stop: async () => {},
  };
  try {
    const view = render(createElement(WslRows, { host, machines: [host], actions }));
    assert.ok(view.getByText("Claude Code"));
    fireEvent.click(view.getByRole("button", { name: "Add Ubuntu as a machine" }));
    assert.ok(await findByText(view.container, "Installing beep-node in Ubuntu"));
    assert.deepEqual(asked, ["desk Ubuntu"]);
    finish();
    assert.ok(await findByText(view.container, "Added. It shows under Machines shortly."));
  } finally {
    cleanup();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
