import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

test("model view sends one command per action and unmounts when another page opens", async () => {
  const dom = new JSDOM("<!doctype html><body><div id='bar' data-mode='window'></div></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, HTMLTextAreaElement: dom.window.HTMLTextAreaElement, MutationObserver: dom.window.MutationObserver, ResizeObserver: class { observe() {} unobserve() {} disconnect() {} }, getComputedStyle: dom.window.getComputedStyle.bind(dom.window), requestAnimationFrame: () => 0, addEventListener: dom.window.addEventListener.bind(dom.window), removeEventListener: dom.window.removeEventListener.bind(dom.window), localStorage: dom.window.localStorage, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const calls: string[] = [];
  const models = { choices: [{ id: "a", name: "Example A", input_price: null, output_price: null, sees_images: true, default: true }, { id: "b", name: "Example B", input_price: null, output_price: null, sees_images: true, default: false }], selected: "a", thinking: "medium", auto: false, auto_available: true };
  let holdAuto = false, releaseAuto: () => void = () => {};
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async (cmd: string) => { calls.push(cmd); if (cmd === "set_auto_model" && holdAuto) await new Promise<void>(resolve => { releaseAuto = resolve; }); return models; }, Channel: class {} }, event: { listen: async () => () => {} } } });
  const { act, fireEvent, screen, waitFor, cleanup } = await import("@testing-library/react");
  const { MODELS, renderModel, modelKeys } = await import("../src/settings/model.ts");
  const { S } = await import("../src/bar/core.ts");
  const { openBarPage } = await import("../src/bar/bar-app.tsx");
  const bar = dom.window.document.getElementById("bar");
  assert.ok(bar);
  MODELS.info = { ...models, thinking: "medium" };
  try {
    act(() => renderModel());
    fireEvent.click(screen.getByRole("switch", { name: "Auto" }));
    await waitFor(() => assert.equal(calls.filter(c => c === "set_auto_model").length, 1));
    act(() => modelKeys(new dom.window.KeyboardEvent("keydown", { key: "ArrowDown", cancelable: true })));
    assert.equal(screen.getByRole("option", { name: /Example B/ }).getAttribute("aria-selected"), "true");
    await act(async () => { await modelKeys(new dom.window.KeyboardEvent("keydown", { key: "Enter", cancelable: true })); });
    assert.equal(calls.filter(c => c === "set_model").length, 1);
    const sharedHost = bar.firstElementChild;
    act(() => { const next = openBarPage(() => {}); next.show({ view: "none" }); });
    assert.equal(bar.firstElementChild, sharedHost);
    act(() => renderModel());
    assert.equal(bar.firstElementChild, sharedHost, "the model view reuses the shared bar root");
    holdAuto = true;
    fireEvent.click(screen.getByRole("switch", { name: "Auto" }));
    await waitFor(() => assert.equal(calls.filter(c => c === "set_auto_model").length, 2));
    let released = false;
    await act(async () => { S.view = "chat"; const next = openBarPage(() => { released = true; }); next.show({ view: "settings-failure", error: "Chat page" }); await Promise.resolve(); });
    assert.equal(screen.queryByRole("switch", { name: "Auto" }), null, "the model view unmounts when the next page opens");
    await act(async () => { releaseAuto(); await Promise.resolve(); });
    assert.equal(screen.queryByRole("switch", { name: "Auto" }), null, "a late reply does not remount the model view");
    assert.ok(screen.getByText("Chat page"));
    assert.equal(released, false);
    act(() => renderModel());
    assert.equal(released, true, "the page the model view replaces is released");
    assert.equal(screen.queryByText("Chat page"), null);
    assert.ok(screen.getByRole("option", { name: /Example A/ }));
  } finally {
    await new Promise(resolve => setTimeout(resolve));
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
