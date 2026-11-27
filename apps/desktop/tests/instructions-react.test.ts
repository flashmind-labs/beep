import { routerWindowGlobals } from "./start-react-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

// Exercise Router-backed views as client code in jsdom.
process.env.NODE_ENV = "test";

test("instructions keep editing and focus while saving and preserve later changes", async () => {
  const dom = new JSDOM("<!doctype html><body><div id='host'></div></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ ...routerWindowGlobals(dom.window), window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, HTMLTextAreaElement: dom.window.HTMLTextAreaElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const initial = { version: 1, text: "first", note: "", saved_at: 0 };
  let finish: (value: typeof initial) => void = () => {};
  let fail: (reason: string) => void = () => {};
  let savedArgs: Record<string, unknown> | null = null;
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async (cmd: string, args: Record<string, unknown>) => {
    if (cmd === "instructions_get") return initial;
    if (cmd === "instructions_save") { savedArgs = args; return new Promise<typeof initial>((resolve, reject) => { finish = resolve; fail = reject; }); }
    if (cmd === "instructions_versions") return [initial];
    return null;
  }, Channel: class {} }, event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) } } });
  const { act, fireEvent, screen, waitFor, cleanup } = await import("@testing-library/react");
  const { invoke } = await import("../src/ui/tauri.ts");
  const { instructionsPage } = await import("../src/screens/settings/instructions.tsx");
  const view = instructionsPage({ invoke, ago: () => "recent", openSettings() {} });
  const host = dom.window.document.getElementById("host");
  assert.ok(host);
  try {
    await act(async () => { await view.mount(host, null); });
    await waitFor(() => assert.ok(screen.getByLabelText("Your instructions")));
    const input = screen.getByLabelText("Your instructions") as HTMLTextAreaElement;
    const { SUGGESTIONS } = await import("../src/stores/settings/instructions-state.ts");
    fireEvent.click(screen.getByRole("button", { name: "Add" }));
    assert.equal(input.value, `first\n\n${SUGGESTIONS[0].text}\n`);
    assert.ok((screen.getByRole("button", { name: "Added" }) as HTMLButtonElement).disabled);
    assert.ok(screen.getByText("Not saved"));
    input.focus();
    fireEvent.change(input, { target: { value: "submitted" } });
    input.setSelectionRange(3, 6);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    assert.equal(screen.getByText("Saving…").textContent, "Saving…");
    assert.equal(screen.getByLabelText("Your instructions"), input);
    fireEvent.change(input, { target: { value: "newer edit" } });
    input.setSelectionRange(2, 5);
    await act(async () => { finish({ ...initial, version: 2, text: "submitted" }); });
    assert.equal(screen.getByLabelText("Your instructions"), input);
    assert.equal(input.value, "newer edit");
    assert.equal(input.selectionStart, 2);
    assert.equal(input.selectionEnd, 5);
    assert.ok(screen.getByText("Not saved"));
    fireEvent.click(screen.getAllByRole("button", { name: "History" })[0]);
    await waitFor(() => assert.ok(screen.getByText("Version 1")));
    fireEvent.click(screen.getByRole("button", { name: "Show" }));
    assert.ok(screen.getByText("first"));
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    assert.deepEqual(savedArgs, { text: "first", base: 2, note: "Restored version 1" });
    await act(async () => { fail("bad request: Save refused"); });
    await waitFor(() => assert.ok(screen.getByLabelText("Your instructions")));
    assert.equal((screen.getByLabelText("Your instructions") as HTMLTextAreaElement).value, "first");
    assert.ok(screen.getByText("Save refused"));
    assert.ok(screen.getByText("Not saved"));
    fireEvent.click(screen.getByRole("button", { name: "Discard" }));
    assert.equal((screen.getByLabelText("Your instructions") as HTMLTextAreaElement).value, "submitted");
    fireEvent.change(screen.getByLabelText("Your instructions"), { target: { value: "pending at close" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    act(() => view.unmount?.());
    await act(async () => { finish({ ...initial, version: 3, text: "pending at close" }); });
    assert.equal(host.querySelector(".in-view"), null, "a late save cannot remount a closed view");
  } finally {
    act(() => view.unmount?.()); cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
