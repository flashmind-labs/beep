import { answers, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act } from "react";
import type { AgentModels } from "../src/agents/agentmodels.ts";
import { drawPicker, type PickerOpts } from "../src/agents/agentpicker.tsx";
import { modelsOn, preloadModels, takeModels } from "../src/agents/model-store.ts";

const list: AgentModels = { pick: "listed", current: "model-a", models: [{ id: "model-a", name: "Model A", description: null, provider: null }] };
const flush = () => new Promise<void>(resolve => setImmediate(resolve));
const requests = () => calls.filter(call => call.cmd === "agent_models").length;

test("a failed lookup is not asked again by a draw, only by the next hosts push", async () => {
  answers.agent_models = null;
  let changes = 0;
  const read = () => modelsOn("retry-host", "codex", () => changes++);
  read();
  await flush();
  assert.equal(read(), null);
  assert.equal(changes, 1);
  const before = requests();
  for (let i = 0; i < 5; i++) read();
  await flush();
  assert.equal(requests(), before, "a draw never asks again");

  answers.agent_models = list;
  preloadModels([{ machine_id: "retry-host", agents: ["codex"] }]);
  await flush();
  assert.equal(requests(), before + 1);
  assert.deepEqual(read(), list);
  preloadModels([{ machine_id: "retry-host", agents: ["codex"] }]);
  await flush();
  assert.equal(requests(), before + 1, "a list that came is left to the pushes");
});

test("Retry reloads immediately, stays open and preserves the model selection", async t => {
  const dom = new JSDOM("<body><div id='picker'></div></body>");
  Object.assign(dom.window, { __TAURI__: window.__TAURI__ });
  // jsdom lays nothing out; the model list draws the rows that fit its height.
  Object.defineProperty(dom.window.HTMLElement.prototype, "clientHeight", { configurable: true, get: () => 340 });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { createRoot } = await import("react-dom/client");
  const host = dom.window.document.getElementById("picker");
  assert.ok(host);
  const root = createRoot(host);
  t.after(async () => {
    await act(() => root.unmount());
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  });
  const options: PickerOpts = {
    render: view => root.render(view),
    picked: { agent: "codex", model: "model-a", effort: null, fast: null, query: "" },
    offered: ["codex"], everywhere: ["codex"], allowed: null, machine: { id: "picker-host", name: "build-box" },
    noMachine: "Pick a machine.", steps: null, anchor: () => null, changed: () => {},
  };
  answers.agent_models = null;
  preloadModels([{ machine_id: "picker-host", agents: ["codex"] }]);
  await flush();
  await act(() => drawPicker(host, options));
  assert.match(host.textContent ?? "", /Couldn't load models/);
  assert.doesNotMatch(host.textContent ?? "", /It runs its own model/);
  const retry = host.querySelector<HTMLButtonElement>("[data-agretry]");
  assert.ok(retry);
  const before = requests();
  answers.agent_models = list;
  act(() => { retry.click(); });
  assert.match(host.textContent ?? "", /Asking the machine/);
  assert.equal(host.querySelector("[data-agretry]"), null);
  await act(flush);
  assert.equal(requests(), before + 1);
  assert.equal(host.querySelector("[data-agretry]"), null);
  assert.equal(host.querySelector('[data-agmo="model-a"]')?.classList.contains("on"), true);
  assert.equal(options.picked.model, "model-a");
});

test("concurrent subscribers share a request and all finish", async () => {
  let finish: (value: AgentModels) => void = () => {};
  answers.agent_models = new Promise<AgentModels>(resolve => { finish = resolve; });
  const before = requests();
  let changes = 0;
  for (let i = 0; i < 2; i++) assert.equal(modelsOn("shared-host", "codex", () => changes++), undefined);
  assert.equal(requests(), before + 1);
  finish(list);
  await flush();
  assert.equal(changes, 2);
  assert.deepEqual(modelsOn("shared-host", "codex", () => {}), list);
});

test("pushed models replace a list without a read, and a failed view keeps the list", async () => {
  const view = (value: AgentModels | null) => ({ views: [{ command: "agent_models", args: { machineId: "pushed-host", agent: "codex" }, value }] });
  const before = requests();
  let changes = 0;
  assert.equal(modelsOn("pushed-host", "codex", () => changes++), undefined);
  takeModels(view(list));
  assert.equal(changes, 1, "the one first read is answered by the push");
  assert.deepEqual(modelsOn("pushed-host", "codex", () => {}), list);

  const next: AgentModels = { ...list, current: "model-b" };
  takeModels(view(next));
  assert.deepEqual(modelsOn("pushed-host", "codex", () => {}), next);

  takeModels({ views: [{ command: "agent_models", args: { machineId: "pushed-host", agent: "codex" }, error: "offline" }] });
  assert.deepEqual(modelsOn("pushed-host", "codex", () => {}), next);
  assert.equal(requests(), before + 1, "only the first read asked");
});
