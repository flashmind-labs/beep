import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import { FlshView } from "../src/settings/flsh-view.tsx";
import { EMPTY, type FlshSettings } from "../src/settings/flsh-state.ts";

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
  }, Channel: class {} }, event: { listen: async () => () => {} } } });
  const { act, render, fireEvent, waitFor, cleanup } = await import("@testing-library/react");
  const { invoke } = await import("../src/ui/tauri.ts");
  const { flshPanel } = await import("../src/settings/flsh.ts");
  const panel = flshPanel(invoke);
  try {
    const view = render(createElement(FlshView, { panel }));
    await waitFor(() => assert.ok(view.getByLabelText("Auto-compact threshold")));
    const input = view.getByLabelText("Auto-compact threshold") as HTMLInputElement;
    input.focus();
    fireEvent.change(input, { target: { value: "300k" } });
    input.setSelectionRange(1, 3);
    fireEvent.click(view.getByRole("button", { name: "Save" }));
    assert.deepEqual(submitted, { ...EMPTY, autocompact: { enabled: true, tokens: 300000 } });
    fireEvent.change(input, { target: { value: "400k" } });
    input.setSelectionRange(1, 3);
    saved = { ...EMPTY, autocompact: { enabled: true, tokens: 300000 } };
    await act(async () => { finish(); await Promise.resolve(); });
    await waitFor(() => assert.equal((view.getByLabelText("Auto-compact threshold") as HTMLInputElement).value, "400k"));
    assert.equal(view.getByLabelText("Auto-compact threshold"), input);
    assert.equal(dom.window.document.activeElement, input);
    assert.equal(input.selectionStart, 1); assert.equal(input.selectionEnd, 3);
    view.unmount();
    const reopened = render(createElement(FlshView, { panel }));
    assert.equal((reopened.getByLabelText("Auto-compact threshold") as HTMLInputElement).value, "400k");
    assert.equal(reads, 2, "remount reuses the loaded state");
    fireEvent.click(reopened.getByRole("button", { name: "Discard" }));
    assert.equal((reopened.getByLabelText("Auto-compact threshold") as HTMLInputElement).value, "300000");
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
