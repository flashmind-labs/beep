import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import type { AgentModels } from "../src/stores/agents/agentmodels.ts";
import { modelContextPanel } from "../src/stores/settings/model-context.ts";

test("model context keeps focus and drafts through updates and saves overrides", async () => {
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, HTMLInputElement: dom.window.HTMLInputElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  let resolveStale: (value: unknown) => void = () => {};
  let contextWindow: number | null = 1000, maxContextWindow: number | null = 2000;
  const calls: { cmd: string; args: Record<string, unknown> | undefined }[] = [];
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async (cmd: string, args: Record<string, unknown> | undefined) => {
    calls.push({ cmd, args });
    if (cmd === "agent_models" && args?.machineId === "stale") return new Promise(resolve => { resolveStale = resolve; });
    if (cmd === "agent_models") return { pick: "listed", current: null, models: [{ id: "alpha", name: "Alpha", description: null, context_window: contextWindow, max_context_window: maxContextWindow }, { id: "beta", name: "Beta", description: null }] };
    if (cmd === "harness_model_compacts") return [];
    return null;
  }, Channel: class {} }, event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) } } });
  const { invoke } = await import("../src/ui/tauri.ts");
  const { render, fireEvent, waitFor, cleanup } = await import("@testing-library/react");
  const { ModelContextView } = await import("../src/screens/settings/model-context-view.tsx");
  const models = (machineId: string, agent: string) => invoke<AgentModels | null>("agent_models", { machineId, agent });
  const panel = modelContextPanel(invoke, models);
  try {
    const view = render(createElement(ModelContextView, { panel, machine: "one", agent: "codex" }));
    const input = await view.findByLabelText("Compact at for alpha") as HTMLInputElement;
    input.focus();
    fireEvent.change(input, { target: { value: "75%" } });
    input.setSelectionRange(1, 2);
    view.rerender(createElement(ModelContextView, { panel, machine: "one", agent: "codex" }));
    assert.equal(view.getByLabelText("Compact at for alpha"), input);
    assert.equal(dom.window.document.activeElement, input);
    assert.equal(input.selectionStart, 1);
    assert.equal(input.selectionEnd, 2);
    fireEvent.click(view.getByText("Save model overrides"));
    await waitFor(() => assert.equal(calls.filter(call => call.cmd === "harness_model_compacts_save").length, 1));
    assert.deepEqual(calls.find(call => call.cmd === "harness_model_compacts_save")?.args, { agent: "codex", rules: [{ model: "alpha", compact: { enabled: true, percent: 75, tokens: null } }] });
    await waitFor(() => assert.equal((view.getByText("Save model overrides") as HTMLButtonElement).disabled, true));
    const defaultInput = view.getByLabelText("Default model") as HTMLInputElement;
    fireEvent.change(defaultInput, { target: { value: "my " } });
    assert.equal(defaultInput.value, "my ");
    fireEvent.change(view.getByLabelText("Filter models"), { target: { value: "nonmatching" } });
    fireEvent.click(view.getByText("Add model"));
    const added = view.getByLabelText("Model id for override") as HTMLInputElement;
    assert.equal(added.closest("tr")?.hidden, false);
    assert.equal(dom.window.document.activeElement, added);
    fireEvent.change(view.getByLabelText("Filter models"), { target: { value: "" } });
    fireEvent.change(added, { target: { value: "custom" } });
    assert.equal(view.getByLabelText("Model id for override"), added);
    assert.equal(dom.window.document.activeElement, added);
    fireEvent.click(view.getByText("Add model"));
    fireEvent.click(view.getByText("Add model"));
    assert.equal(view.getAllByLabelText("Model id for override").length, 3);
    view.rerender(createElement(ModelContextView, { panel, machine: "two", agent: "codex" }));
    await view.findByLabelText("Compact at for alpha");
    view.rerender(createElement(ModelContextView, { panel, machine: "one", agent: "codex" }));
    assert.equal((view.getAllByLabelText("Model id for override")[0] as HTMLInputElement).value, "custom");
    view.unmount();
    const staleLoad = panel.load("stale", "codex");
    await panel.save("one", "codex");
    resolveStale({ pick: "listed", current: null, models: [] });
    await staleLoad;
    assert.equal(panel.snapshot("stale", "codex").loaded, false);

    for (const [defaultLimit, maxLimit] of [[null, null], [1000, null], [null, 2000], [1000, 2000]] as const) {
      contextWindow = defaultLimit;
      maxContextWindow = maxLimit;
      const limitsView = render(createElement(ModelContextView, { panel: modelContextPanel(invoke, models), machine: "one", agent: "claude" }));
      await limitsView.findByLabelText("Compact at for alpha");
      assert.equal(!!limitsView.queryByRole("columnheader", { name: "Default usable" }), defaultLimit !== null);
      assert.equal(!!limitsView.queryByRole("columnheader", { name: "Maximum usable" }), maxLimit !== null);
      assert.equal(!!limitsView.queryByText(/Limits from this machine's harness/), defaultLimit !== null || maxLimit !== null);
      for (const row of limitsView.container.querySelectorAll("tbody tr")) {
        assert.equal(row.children.length, limitsView.getAllByRole("columnheader").length);
      }
      limitsView.unmount();
    }

    contextWindow = null;
    maxContextWindow = null;
    const unsupported = render(createElement(ModelContextView, { panel: modelContextPanel(invoke, models), machine: "one", agent: "agy" }));
    await unsupported.findByLabelText("Default model");
    assert.equal(unsupported.queryAllByRole("columnheader").length, 1);
    assert.equal(unsupported.queryByText("Compact at"), null);
    assert.equal(unsupported.queryByText("Save model overrides"), null);
    assert.equal(unsupported.queryByText(/Overrides apply/), null);
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
