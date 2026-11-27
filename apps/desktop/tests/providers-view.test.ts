import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement, useState, type ComponentProps } from "react";
import { modelContextPanel } from "../src/stores/settings/model-context.ts";
import { flshPanel } from "../src/stores/settings/flsh.ts";
import { draftOf, type HarnessView } from "../src/stores/settings/providers-state.ts";

test("coding agent runtime keeps focused drafts and dispatches nested actions", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { ProvidersView } = await import("../src/screens/settings/providers-view.tsx");
  const { render, fireEvent, cleanup } = await import("@testing-library/react");
  const hv: HarnessView = { agent: "codex", installed: true, runnable: true, version: "1.0", latest: null, program: "codex", home_var: null, home_default: null, sign_in: { state: "signed_in", account: null, plan: null, last4: null }, install: null, settings: { enabled: true, program: "codex", home: "", args: [], env: [] } };
  let toggled = 0, picked = 0, saved = 0;
  const noOp = () => {};
  const props: ComponentProps<typeof ProvidersView> = { loadOthers: noOp, loadCompact: noOp, changeCompact: noOp, saveCompact: noOp, discardCompact: noOp, machines: [{ id: "one", name: "build-box", online: true }], machine: "one", machineName: "build-box", view: { machine_id: "one", machine_name: "build-box", checked_at: 0, harnesses: [hv] }, picked: "codex", draft: null, others: false, saving: false, updating: false, adapter: null, error: "", inSettings: false, savedAll: "", includeProgram: false, shared: null, contexts: modelContextPanel(async () => { throw new Error("unexpected call"); }, async () => { throw new Error("unexpected call"); }), flsh: flshPanel(async () => { throw new Error("unexpected call"); }), invoke: async () => { throw new Error("unexpected call"); }, loading: false, checked: "Check", updateCount: 0, canUpdate: false,
    pick: () => { picked++; }, toggle: () => { toggled++; }, update: noOp, retryAdapter: noOp, openAccounts: noOp, setDraft: noOp, save: () => { saved++; }, saveAll: noOp, discard: noOp, setIncludeProgram: noOp, dismissError: noOp, pickMachine: noOp, refresh: noOp, updateAll: noOp };
  function Harness({ error }: { error: string }) {
    const [draft, setDraft] = useState(() => draftOf(hv));
    return createElement(ProvidersView, { ...props, error, draft, setDraft: change => setDraft(old => ({ ...old, ...change })) });
  }
  try {
    const view = render(createElement(Harness, { error: "" }));
    const program = view.getByLabelText("Binary path") as HTMLInputElement;
    program.focus();
    fireEvent.change(program, { target: { value: "/path/to/agent" } });
    program.setSelectionRange(2, 6);
    const scroller = view.container.querySelector<HTMLElement>(".hv-view");
    assert.ok(scroller);
    scroller.scrollTop = 47;
    view.rerender(createElement(Harness, { error: "Connection lost" }));
    assert.equal(view.getByLabelText("Binary path"), program);
    assert.equal(program.value, "/path/to/agent");
    assert.equal(dom.window.document.activeElement, program);
    assert.equal(program.selectionStart, 2);
    assert.equal(program.selectionEnd, 6);
    assert.equal(scroller.scrollTop, 47);
    const toggle = view.getByRole("switch", { name: "Codex on build-box" });
    fireEvent.keyDown(toggle, { key: "Enter" });
    fireEvent.click(toggle);
    assert.equal(toggled, 1);
    assert.equal(picked, 0);
    fireEvent.click(view.getByRole("button", { name: "Save" }));
    assert.equal(saved, 1);
    view.getByLabelText("Variable name").focus();
    fireEvent.change(view.getByLabelText("Variable name"), { target: { value: "EXAMPLE" } });
    assert.equal(view.getAllByLabelText("Variable name").length, 2);
    assert.equal(dom.window.document.activeElement?.getAttribute("aria-label"), "Variable name");
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
