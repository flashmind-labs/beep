import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { createElement, type ComponentProps } from "react";

test("machine setup preserves the project folder and manual ticket while updates arrive", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { AddMachineBody } = await import("../src/screens/machines/addmachine-view.tsx");
  const { render, fireEvent, cleanup } = await import("@testing-library/react");
  let picked = "", useMac = 0, closed = 0;
  const props: ComponentProps<typeof AddMachineBody> = { state: { step: "waiting", token: "example", command: "join", expiresAt: 120000 }, kind: "node", mac: null, macAsked: false, now: 0,
    selectKind: kind => { picked = kind; }, useThisMac: () => { useMac++; }, start: () => {}, close: () => { closed++; } };
  try {
    const view = render(createElement(AddMachineBody, props));
    const input = view.getByLabelText("Search for projects in") as HTMLInputElement;
    input.focus();
    fireEvent.change(input, { target: { value: "/path/to/work" } });
    assert.match(view.container.querySelector(".am-cmd")?.textContent ?? "", /join --search \/path\/to\/work/);
    view.rerender(createElement(AddMachineBody, { ...props, now: 1000, mac: { enabled: false, status: "connected", error: null } }));
    assert.equal(view.getByLabelText("Search for projects in"), input);
    assert.equal(dom.window.document.activeElement, input);
    assert.equal(input.value, "/path/to/work");
    fireEvent.click(view.getByRole("button", { name: "Use this computer" }));
    assert.equal(useMac, 1);
    fireEvent.click(view.getByRole("radio", { name: /Phone/ }));
    assert.equal(picked, "phone");
    const phone: ComponentProps<typeof AddMachineBody> = { ...props, kind: "phone", state: { step: "waiting", token: "phone", command: "ticket", expiresAt: 120000, qrSvg: "<svg></svg>" } };
    view.rerender(createElement(AddMachineBody, phone));
    const details = view.container.querySelector<HTMLDetailsElement>("details");
    assert.ok(details);
    details.open = true;
    view.rerender(createElement(AddMachineBody, { ...phone, now: 1000 }));
    assert.equal(view.container.querySelector("details"), details);
    assert.equal(details.open, true);
    fireEvent.click(view.getByRole("button", { name: "Cancel" }));
    assert.equal(closed, 1);
    view.rerender(createElement(AddMachineBody, { ...props, state: { step: "expired" } }));
    assert.ok(view.getByText(/ticket expired/));
  } finally {
    cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
