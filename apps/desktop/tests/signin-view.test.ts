import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";
import type { SignInView } from "../src/stores/machines/signin-state.ts";

test("sign-in updates keep the typed code and focused input until it is sent", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { SignInBody } = await import("../src/screens/machines/signin-view.tsx");
  const { render, fireEvent, act, waitFor, cleanup } = await import("@testing-library/react");
  let release: () => void = () => {}, submitted = "";
  const send = (text: string) => { submitted = text; return new Promise<void>(resolve => { release = resolve; }); };
  const view: SignInView = { state: "waiting", url: "https://example.test/login", code: "ABCD", asks_code: true, reason: null };
  const noOp = () => {};
  const props = { machineName: "build-box", tool: "agent", view, error: "", sending: false, send, retry: noOp, close: noOp, openUrl: noOp };
  try {
    const rendered = render(createElement(SignInBody, props));
    const input = rendered.getByPlaceholderText("Paste the code") as HTMLInputElement;
    assert.equal(dom.window.document.activeElement, input);
    fireEvent.change(input, { target: { value: "  user code  " } });
    rendered.rerender(createElement(SignInBody, { ...props, view: { ...view, code: "EFGH" } }));
    assert.equal(rendered.getByPlaceholderText("Paste the code"), input);
    assert.equal(input.value, "  user code  ");
    assert.equal(dom.window.document.activeElement, input);
    fireEvent.keyDown(input, { key: "Enter" });
    assert.equal(submitted, "  user code  ");
    rendered.rerender(createElement(SignInBody, { ...props, sending: true }));
    assert.ok(rendered.getByText("Sending…"));
    await act(async () => { release(); await Promise.resolve(); });
    await waitFor(() => assert.equal(input.value, ""));
    rendered.rerender(createElement(SignInBody, { ...props, view: { ...view, state: "failed", reason: "Sign-in failed" } }));
    assert.ok(rendered.getByText("Sign-in failed"));
    assert.ok(rendered.getByText("Try again"));
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
