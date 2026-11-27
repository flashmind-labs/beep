import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

test("Try again is usable after the issue fails to open a second time", async () => {
  const dom = new JSDOM("<!doctype html><body></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const draft = { manifest: { version: 1, sender: "x", mesh: null, app: "desktop", app_version: "0.1", text: "", period: "today", since: 0, made_at: 0, machines: [], files: [{ path: "log.txt", bytes: 3 }] }, files: [{ path: "log.txt", text: "abc" }] };
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async (cmd: string) => {
    if (cmd === "report_collect") return draft;
    if (cmd === "report_send") return { reference: "R-1", gh: true };
    if (cmd === "report_issue_text") return "issue body";
    if (cmd === "report_issue") throw new Error("gh failed");
    return null;
  }, Channel: class {} }, event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) } } });
  const { act, fireEvent, screen, waitFor, cleanup } = await import("@testing-library/react");
  const { openReport } = await import("../src/screens/report/report.tsx");

  try {
    act(() => { openReport(dom.window.document.body); });
    fireEvent.change(screen.getByRole("textbox"), { target: { value: "It broke" } });
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Collect" })); });
    await waitFor(() => screen.getByRole("button", { name: "Send" }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Send" })); });
    await waitFor(() => assert.equal((screen.getByRole("button", { name: "Yes" }) as HTMLButtonElement).disabled, false));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Yes" })); });
    await waitFor(() => screen.getByRole("button", { name: "Try again" }));
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "Try again" })); });
    await waitFor(() => screen.getByRole("button", { name: "Try again" }));
    assert.equal((screen.getByRole("button", { name: "Try again" }) as HTMLButtonElement).disabled, false);
  } finally {
    cleanup();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
