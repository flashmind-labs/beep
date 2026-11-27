import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

test("a late invitation from a closed sheet cannot replace the new ticket", async t => {
  t.mock.timers.enable({ apis: ["setInterval"] });
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  type Ticket = { token: string; command: string; qr_svg: null; expires_at: number };
  const pending: ((ticket: Ticket) => void)[] = [];
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async (cmd: string) => {
    if (cmd === "mesh_invite") return new Promise<Ticket>(resolve => { pending.push(resolve); });
    if (cmd === "mesh_watch") return new Promise(() => {});
    if (cmd === "mac_control") return { enabled: true, status: "connected", error: null };
    return null;
  }, Channel: class {} }, event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) } } });
  const { act, fireEvent, screen, waitFor, cleanup } = await import("@testing-library/react");
  const { openAddMachine, closeAddMachine } = await import("../src/screens/machines/addmachine.tsx");
  let closed = 0;
  try {
    act(() => { openAddMachine({ onClose: () => { closed++; } }); });
    await waitFor(() => assert.equal(pending.length, 1));
    act(() => closeAddMachine());
    act(() => { openAddMachine({ onClose: () => { closed++; } }); });
    await waitFor(() => assert.equal(pending.length, 2));
    await act(async () => { pending[1]({ token: "new", command: "new command", qr_svg: null, expires_at: Date.now() + 60000 }); });
    assert.ok(screen.getByText("new command"));
    const outside = dom.window.document.createElement("input");
    dom.window.document.body.append(outside);
    outside.focus();
    act(() => t.mock.timers.tick(1000));
    assert.equal(dom.window.document.activeElement, outside, "a countdown tick does not steal focus");
    await act(async () => { pending[0]({ token: "old", command: "old command", qr_svg: null, expires_at: Date.now() + 60000 }); });
    assert.equal(screen.queryByText("old command"), null);
    assert.ok(screen.getByText("new command"));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    assert.equal(closed, 2);
  } finally {
    act(() => closeAddMachine()); cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
