import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

test("model view sends one command per action and unmounts when the bar changes", async () => {
  const dom = new JSDOM("<!doctype html><body><div id='bar' data-mode='window'></div></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, HTMLTextAreaElement: dom.window.HTMLTextAreaElement, MutationObserver: dom.window.MutationObserver, ResizeObserver: class { observe() {} unobserve() {} disconnect() {} }, requestAnimationFrame: () => 0, addEventListener: dom.window.addEventListener.bind(dom.window), removeEventListener: dom.window.removeEventListener.bind(dom.window), localStorage: dom.window.localStorage, IS_REACT_ACT_ENVIRONMENT: true })) {
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
    holdAuto = true;
    fireEvent.click(screen.getByRole("switch", { name: "Auto" }));
    await waitFor(() => assert.equal(calls.filter(c => c === "set_auto_model").length, 2));
    const host = bar.firstElementChild;
    assert.ok(host);
    await act(async () => { S.view = "chat"; bar.replaceChildren(); await Promise.resolve(); });
    assert.equal(host.childElementCount, 0, "the detached React tree is unmounted");
    await act(async () => { releaseAuto(); await Promise.resolve(); });
    assert.equal(bar.childElementCount, 0, "a late reply does not remount the model view");
    act(() => renderModel());
    assert.ok(screen.getByRole("option", { name: /Example A/ }));
  } finally {
    await act(async () => { bar.replaceChildren(); await Promise.resolve(); });
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
