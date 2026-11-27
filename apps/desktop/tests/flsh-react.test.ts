import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import { EMPTY, type FlshSettings } from "../src/stores/settings/flsh-state.ts";

test("flsh routing preserves edits and focus during save and reuses the panel on remount", async () => {
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, HTMLInputElement: dom.window.HTMLInputElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  let saved: FlshSettings = { ...EMPTY }, finish: () => void = () => {}, submitted: unknown = null, reads = 0;
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async (cmd: string, args: Record<string, unknown>) => {
    if (cmd === "flsh_settings_get") { reads++; return saved; }
    if (cmd === "flsh_settings_save") { submitted = args.settings; return new Promise<void>(resolve => { finish = resolve; }); }
    return null;
  }, Channel: class {} }, event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) } } });
  const { act, render, fireEvent, waitFor, cleanup, within } = await import("@testing-library/react");
  const { FlshView } = await import("../src/screens/settings/flsh-view.tsx");
  const { invoke } = await import("../src/ui/tauri.ts");
  const { flshPanel } = await import("../src/stores/settings/flsh.ts");
  const panel = flshPanel(invoke);
  try {
    const view = render(createElement(FlshView, { panel, agent: "flsh" }));
    await waitFor(() => assert.ok(view.getByLabelText("Auto-compact threshold")));
    const input = view.getByLabelText("Auto-compact threshold") as HTMLInputElement;
    fireEvent.change(view.getByLabelText("Stream idle timeout (seconds)"), { target: { value: "120" } });
    input.focus();
    fireEvent.change(input, { target: { value: "300k" } });
    input.setSelectionRange(1, 3);
    fireEvent.click(view.getByRole("button", { name: "Save" }));
    assert.deepEqual(submitted, { ...EMPTY, stream_idle_timeout_secs: 120, autocompact: { enabled: true, tokens: 300000 } });
    fireEvent.change(input, { target: { value: "400k" } });
    input.setSelectionRange(1, 3);
    saved = { ...EMPTY, stream_idle_timeout_secs: 120, autocompact: { enabled: true, tokens: 300000 } };
    await act(async () => { finish(); await Promise.resolve(); });
    await waitFor(() => assert.equal((view.getByLabelText("Auto-compact threshold") as HTMLInputElement).value, "400k"));
    assert.equal(view.getByLabelText("Auto-compact threshold"), input);
    assert.equal(dom.window.document.activeElement, input);
    assert.equal(input.selectionStart, 1); assert.equal(input.selectionEnd, 3);
    view.unmount();
    const reopened = render(createElement(FlshView, { panel, agent: "flsh" }));
    assert.equal((reopened.getByLabelText("Auto-compact threshold") as HTMLInputElement).value, "400k");
    assert.equal(reads, 2, "remount reuses the loaded state");
    fireEvent.click(reopened.getByRole("button", { name: "Discard" }));
    assert.equal((reopened.getByLabelText("Auto-compact threshold") as HTMLInputElement).value, "300000");
    reopened.unmount();
    // pi takes the routing only: a model's quantizations, no compaction.
    const pi = render(createElement(FlshView, { panel, agent: "pi" }));
    assert.equal(pi.queryByLabelText("Auto-compact threshold"), null);
    assert.equal(pi.queryByLabelText("Stream idle timeout (seconds)"), null);
    fireEvent.click(pi.getByRole("button", { name: "Add model" }));
    assert.equal(pi.queryByLabelText("Auto-compact for model 1"), null);
    fireEvent.change(pi.getByLabelText("Model 1"), { target: { value: "openrouter:vendor/fp4-only" } });
    fireEvent.click(within(pi.getByRole("group", { name: "Allowed quantizations for model 1" })).getByRole("button", { name: "fp4" }));
    fireEvent.click(pi.getByRole("button", { name: "Save" }));
    assert.deepEqual(submitted, { ...saved, models: [{ model: "openrouter:vendor/fp4-only", inherits_compaction: true, enabled: true, tokens: null, quantizations: ["fp4"] }] });
    await act(async () => { finish(); await Promise.resolve(); });
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
