import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement } from "react";

test("desktop linking validates input, prevents duplicate joins and retries failures", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { render, fireEvent, waitFor, cleanup, act } = await import("@testing-library/react");
  const { JoinTicketFields } = await import("../src/screens/machines/jointicket-view.tsx");
  let calls = 0, value = "", done = 0, reject: (reason: Error) => void = () => {};
  const join = (ticket: string) => {
    calls++; value = ticket;
    if (calls > 1) return Promise.resolve();
    return new Promise<void>((_resolve, fail) => { reject = fail; });
  };
  try {
    const view = render(createElement(JoinTicketFields, { done: () => { done++; }, join, back: () => {}, backLabel: "Back" }));
    const input = view.getByLabelText("Desktop ticket");
    const submit = view.getByRole("button", { name: "Link" }) as HTMLButtonElement;
    assert.equal(dom.window.document.activeElement, input);
    assert.equal(submit.disabled, true);
    fireEvent.change(input, { target: { value: "  ticket  " } });
    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() => assert.equal(view.getByText("Linking").textContent, "Linking"));
    assert.equal(submit.disabled, true);
    fireEvent.keyDown(input, { key: "Enter" });
    assert.equal(calls, 1);
    assert.equal(value, "  ticket  ");
    reject(new Error("ticket expired"));
    await waitFor(() => assert.match(view.container.querySelector(".am-msg.bad")?.textContent ?? "", /ticket expired/));
    assert.equal(submit.disabled, false);
    fireEvent.click(submit);
    await waitFor(() => assert.equal(done, 1));
    assert.equal(calls, 2);
    view.unmount();

    let release: () => void = () => {};
    const slow = () => new Promise<void>(resolve => { release = resolve; });
    const second = render(createElement(JoinTicketFields, { done: () => { done++; }, join: slow, back: () => second.unmount(), backLabel: "Back" }));
    fireEvent.change(second.getByLabelText("Desktop ticket"), { target: { value: "later" } });
    fireEvent.click(second.getByRole("button", { name: "Link" }));
    fireEvent.click(second.getByRole("button", { name: "Back" }));
    await act(async () => { release(); await Promise.resolve(); });
    assert.equal(done, 1, "leaving the form ignores a late successful join");
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
