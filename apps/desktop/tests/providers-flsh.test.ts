import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { readFileSync } from "node:fs";
import type { CenterView } from "../src/center/state.ts";
import { EMPTY } from "../src/settings/flsh-state.ts";

test("flsh input survives provider machine updates without remounting", async () => {
  const dom = new JSDOM("<!doctype html><body><div id='host'></div></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, HTMLInputElement: dom.window.HTMLInputElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const hv = { agent: "flsh", installed: true, runnable: true, version: "1.0", latest: null, program: "flsh", home_var: null, home_default: null, sign_in: { state: "signed_in", account: null, plan: null, last4: null }, install: null, settings: { enabled: true, program: "flsh", home: "", args: [], env: [] } };
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async (cmd: string) => {
    if (cmd === "list_machines") return [{ id: "one", name: "build-box", online: true }, { id: "two", name: "gpu-box", online: true }];
    if (cmd === "agent_harnesses") return { machine_id: "one", machine_name: "build-box", checked_at: 0, harnesses: [hv] };
    if (cmd === "flsh_settings_get") return EMPTY;
    if (cmd === "harness_settings_all" || cmd === "harness_model_compacts") return [];
    return null;
  }, Channel: class {} }, event: { listen: async () => () => {} } } });
  const { act, fireEvent, screen, waitFor, cleanup } = await import("@testing-library/react");
  const { invoke } = await import("../src/ui/tauri.ts");
  const { el, button } = await import("../src/ui/dom.ts");
  const { registerProviders } = await import("../src/settings/providers.ts");
  const views: CenterView<"providers">[] = [];
  registerProviders({ registerView: (_name, view) => { views.push(view); }, invoke, h: el, button, openSettings() {}, redrawTop() {} });
  const host = dom.window.document.getElementById("host"), view = views[0];
  assert.ok(host); assert.ok(view);
  try {
    await act(async () => { await view.mount(host, { agent: "flsh", settings: true }); });
    await waitFor(() => assert.ok(screen.getByLabelText("Auto-compact threshold")));
    const css = readFileSync(new URL("../dist/center.css", import.meta.url), "utf8");
    const headingRule = css.match(/\.hv-scope > h3\s*\{[^}]+\}/)?.[0];
    assert.ok(headingRule);
    const style = dom.window.document.createElement("style");
    style.textContent = headingRule;
    dom.window.document.head.append(style);
    const heading = screen.getByRole("heading", { name: "All machines" });
    const computed = dom.window.getComputedStyle(heading);
    assert.equal(computed.marginBottom, "12px");
    assert.equal(computed.fontSize, "16px");
    assert.equal(computed.lineHeight, "24px");
    assert.equal(computed.fontWeight, "600");
    const input = screen.getByLabelText("Auto-compact threshold") as HTMLInputElement;
    input.focus();
    fireEvent.change(input, { target: { value: "400k" } });
    input.setSelectionRange(1, 3);
    fireEvent.change(screen.getByLabelText("Machine runtime settings"), { target: { value: "two" } });
    await waitFor(() => assert.equal((screen.getByLabelText("Machine runtime settings") as HTMLSelectElement).value, "two"));
    assert.equal(screen.getByLabelText("Auto-compact threshold"), input);
    assert.equal(input.value, "400k");
    assert.equal(dom.window.document.activeElement, input);
    assert.equal(input.selectionStart, 1); assert.equal(input.selectionEnd, 3);
  } finally {
    act(() => view.unmount?.()); cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
