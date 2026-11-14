import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import type { MachineView, WslDistro } from "../src/machines/machine-state.ts";
import { WslWorkStore, wslKey } from "../src/machines/wsl-state.ts";

test("WSL rows show the work kept apart from them, and offer turning an off node on", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { WslRows } = await import("../src/machines/wsl-view.tsx");
  const { render, fireEvent, act, cleanup } = await import("@testing-library/react");
  const distro = (name: string, over: Partial<WslDistro> = {}): WslDistro => ({ name, agents: ["claude"], node: null, machine_id: null, kept: false, problem: null, probed: true, ...over });
  const host: MachineView = { id: "desk", name: "desk-pc", online: true, seen_at: 0, version: "1.0", env: { os: "windows", os_version: "Windows", arch: "x86_64", hostname: "desk-pc", cpus: 2, memory_mb: 1024, home: "/path/to/work", tools: [], logins: [], avds: [], gpus: [], wsl: [distro("Ubuntu"), distro("Debian", { machine_id: "wsl-box" })] } };
  const wslBox: MachineView = { id: "wsl-box", name: "Debian on desk-pc", online: false, seen_at: 0, version: "1.0", env: null };
  const asked: string[] = [];
  const work = new WslWorkStore();
  const actions = {
    work,
    add: (machine: string, name: string) => { asked.push(`add ${machine} ${name}`); },
    stop: () => {},
    start: (machine: string, name: string) => { asked.push(`start ${machine} ${name}`); },
    probe: (machine: string) => { asked.push(`probe ${machine}`); },
  };
  try {
    const view = render(createElement(WslRows, { host, machines: [host, wslBox], actions }));
    assert.ok(view.getAllByText("Claude Code").length === 2);
    assert.ok(view.getByText("Off"));
    fireEvent.click(view.getByRole("button", { name: "Add Ubuntu as a machine" }));
    fireEvent.click(view.getByRole("button", { name: "Turn on" }));
    assert.deepEqual(asked, ["probe desk", "add desk Ubuntu", "start desk Debian"]);
    act(() => work.set(wslKey("desk", "Ubuntu"), { step: "busy", text: "Installing beep-node in Ubuntu" }));
    assert.ok(view.getByText("Installing beep-node in Ubuntu"));
    act(() => work.set(wslKey("desk", "Ubuntu"), { step: "failed", error: "WSL needs curl or wget" }));
    assert.ok(view.getByText("WSL needs curl or wget"));
    assert.ok(view.getByRole("button", { name: "Add Ubuntu as a machine" }));
  } finally {
    cleanup();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
