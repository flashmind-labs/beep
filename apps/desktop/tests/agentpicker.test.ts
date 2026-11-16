import { answers, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act } from "react";
import type { AgentModels } from "../src/agents/agentmodels.ts";
import { MODELS_KEPT_MS } from "../src/agents/agentmodels.ts";
import { drawPicker, modelsOn, preloadModels, type PickerOpts } from "../src/agents/agentpicker.tsx";

const list: AgentModels = { pick: "listed", current: "model-a", models: [{ id: "model-a", name: "Model A", description: null, provider: null }] };
const flush = () => new Promise<void>(resolve => setImmediate(resolve));
const requests = () => calls.filter(call => call.cmd === "agent_models").length;

test("failed lookups retry after ten seconds and notify again when the answer is unchanged", async t => {
  let now = 1_000_000;
  t.mock.method(Date, "now", () => now);
  answers.agent_models = null;
  let changes = 0;
  const read = () => modelsOn("retry-host", "codex", () => changes++);
  read();
  await flush();
  assert.equal(read(), null);
  assert.equal(changes, 1);
  const before = requests();
  now += 9_000;
  read();
  assert.equal(requests(), before);
  now += 1_001;
  read();
  await flush();
  assert.equal(requests(), before + 1);
  assert.equal(changes, 2, "a repeated failure finishes the loading state");
  now += 10_001;
  answers.agent_models = list;
  read();
  await flush();
  assert.deepEqual(read(), list);
  assert.equal(changes, 3);
});

test("failed refreshes retain the previous list and retry sooner than successful lookups", async t => {
  let now = 2_000_000;
  t.mock.method(Date, "now", () => now);
  answers.agent_models = list;
  const read = () => modelsOn("refresh-host", "codex", () => {});
  read();
  await flush();
  const before = requests();
  now += MODELS_KEPT_MS - 1;
  assert.deepEqual(read(), list);
  assert.equal(requests(), before);
  now += 2;
  answers.agent_models = Promise.reject(new Error("connection lost"));
  read();
  await flush();
  assert.deepEqual(read(), list);
  assert.ok(calls.some(call => call.cmd === "log_webview" && JSON.stringify(call.args).includes("harness=codex: Error: connection lost")));
  now += 10_001;
  answers.agent_models = list;
  read();
  await flush();
  assert.equal(requests(), before + 2);
});

test("Retry reloads immediately, stays open and preserves the model selection", async t => {
  t.mock.method(Date, "now", () => 3_000_000);
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
    offered: ["codex"], everywhere: ["codex"], machine: { id: "picker-host", name: "build-box" },
    noMachine: "Pick a machine.", anchor: () => null, bounds: () => null, changed: () => {},
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

test("concurrent refresh subscribers share a request and all finish when the list is unchanged", async t => {
  let now = 4_000_000;
  t.mock.method(Date, "now", () => now);
  answers.agent_models = list;
  modelsOn("shared-host", "codex", () => {});
  await flush();
  now += MODELS_KEPT_MS + 1;
  let finish: (value: AgentModels) => void = () => {};
  answers.agent_models = new Promise<AgentModels>(resolve => { finish = resolve; });
  const before = requests();
  let changes = 0;
  for (let i = 0; i < 2; i++) {
    assert.deepEqual(modelsOn("shared-host", "codex", () => changes++), list);
  }
  assert.equal(requests(), before + 1);
  finish(list);
  await flush();
  assert.equal(changes, 2);
});
