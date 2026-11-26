import { routerWindowGlobals } from "./start-react-env.ts";
import { answers, calls } from "./dom-env.ts";
import { JSDOM } from "jsdom";
import { MACHINES } from "../src/machines/machine-store.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { invoke } from "../src/ui/tauri.ts";
import type { HarnessView } from "../src/settings/providers-state.ts";

// Exercise Router-backed views as client code in jsdom.
process.env.NODE_ENV = "test";

const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
Reflect.set(dom.window, "__TAURI__", Reflect.get(globalThis, "__TAURI__"));
Object.assign(globalThis, { ...routerWindowGlobals(dom.window), window: dom.window, document: dom.window.document,
  Node: dom.window.Node, HTMLElement: dom.window.HTMLElement, HTMLInputElement: dom.window.HTMLInputElement });
const settle = () => new Promise(resolve => setImmediate(resolve));
const harness: HarnessView = {
  agent: "claude", installed: true, runnable: true, version: "1.0.0", latest: null,
  program: "claude", home_var: "CLAUDE_CONFIG_DIR", home_default: "~/.claude",
  sign_in: { state: "signed_in", account: null, plan: null, last4: null }, install: null,
  settings: { enabled: true, program: "", home: "", args: [], env: [] },
};

test("machine selection preserves shared drafts and each save writes only its scope", async () => {
  answers.list_machines = [{ id: "build", name: "build-box", online: true }, { id: "gpu", name: "gpu-box", online: true }];
  answers.agent_harnesses = { machine_id: "build", machine_name: "build-box", checked_at: null, harnesses: [harness] };
  answers.harness_settings_all = [];
  answers.harness_compact = null;
  answers.harness_model_compacts = [];
  answers.harness_default_model = null;
  answers.harness_driver = "acp";
  answers.agent_models = null;
  const root = document.createElement("div");
  document.body.append(root);
  const { providersPage } = await import("../src/settings/providers.tsx");
  const { fireEvent } = await import("@testing-library/react");
  void providersPage({ invoke, redrawTop: () => {}, openSettings: () => {}, follow: () => {} }).mount(root, { settings: true, agent: "claude" });
  await settle();
  const scopes = () => root.querySelectorAll<HTMLElement>(".hv-scope");
  assert.match(scopes()[0].textContent ?? "", /All machines/);
  assert.equal(scopes()[0].querySelector(".hv-pick"), null);
  assert.ok(scopes()[1].querySelector(".hv-pick"));
  assert.equal(scopes()[1].querySelector(":scope > h3"), null);
  assert.equal(scopes()[1].querySelector(".hv-bar .lbl"), null);
  const threshold = root.querySelector<HTMLInputElement>('[aria-label="Auto-compact threshold"]');
  assert.ok(threshold);
  threshold.focus();
  fireEvent.change(threshold, { target: { value: "70%" } });
  threshold.setSelectionRange(1, 2);
  const picker = [...scopes()[1].querySelectorAll<HTMLButtonElement>(".hv-seg button")].find(button => button.textContent === "gpu-box");
  assert.ok(picker);
  fireEvent.click(picker);
  await settle();
  const restored = root.querySelector<HTMLInputElement>('[aria-label="Auto-compact threshold"]');
  assert.equal(restored, threshold);
  assert.equal(restored?.value, "70%");
  assert.equal(document.activeElement, restored);
  assert.equal(restored?.selectionStart, 1);
  assert.equal(restored?.selectionEnd, 2);
  const program = root.querySelector<HTMLInputElement>('[aria-label="Binary path"]');
  assert.ok(program);
  fireEvent.change(program, { target: { value: "custom-agent" } });
  const findButton = (scope: Element, label: string) => {
    const found = [...scope.querySelectorAll("button")].find(b => b.textContent === label);
    assert.ok(found);
    return found;
  };
  calls.length = 0;
  findButton(scopes()[1], "Save").click();
  await settle();
  assert.ok(calls.some(c => c.cmd === "harness_settings_save" && JSON.stringify(c.args).includes('"machineId":"gpu"')));
  assert.equal(calls.some(c => c.cmd === "harness_compact_save"), false);
  calls.length = 0;
  findButton(scopes()[0], "Save auto-compact").click();
  await settle();
  assert.deepEqual(calls.find(c => c.cmd === "harness_compact_save")?.args, { agent: "claude", compact: { enabled: true, percent: 70, tokens: null } });
  assert.equal(calls.some(c => c.cmd === "harness_settings_save"), false);
});

test("shared compaction stays editable when no machine is online", async () => {
  answers.list_machines = [];
  Object.assign(MACHINES, { list: null });
  const root = document.createElement("div");
  document.body.replaceChildren(root);
  const { providersPage } = await import("../src/settings/providers.tsx");
  void providersPage({ invoke, redrawTop: () => {}, openSettings: () => {}, follow: () => {} }).mount(root, { settings: true, agent: "claude" });
  await settle();
  const threshold = root.querySelector<HTMLInputElement>('[aria-label="Auto-compact threshold"]');
  assert.ok(threshold);
  assert.equal(threshold.disabled, false);
  assert.match(root.textContent ?? "", /No machine online/);
});
