import { routerWindowGlobals } from "./start-react-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

// Exercise Router-backed views as client code in jsdom.
process.env.NODE_ENV = "test";

test("provider save retains edits made while the command is in flight", async () => {
  const dom = new JSDOM("<!doctype html><body><div id='host'></div></body>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ ...routerWindowGlobals(dom.window), window: dom.window, document: dom.window.document, navigator: dom.window.navigator, HTMLElement: dom.window.HTMLElement, HTMLInputElement: dom.window.HTMLInputElement, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const hv = { agent: "codex", installed: true, runnable: true, version: "1.0", latest: null, program: "codex", home_var: null, home_default: null, sign_in: { state: "signed_in", account: null, plan: null, last4: null }, install: null, settings: { enabled: true, program: "codex", home: "", args: [], env: [] } };
  let finish: () => void = () => {};
  const reads: string[] = [];
  let savedProgram = "codex";
  Object.defineProperty(dom.window, "__TAURI__", { value: { core: { invoke: async (cmd: string, args: Record<string, unknown>) => {
    if (cmd === "list_machines") return [{ id: "one", name: "build-box", online: true }, { id: "two", name: "gpu-box", online: true }];
    if (cmd === "agent_harnesses") {
      const machine = String(args.machineId);
      reads.push(machine);
      return { machine_id: machine, machine_name: machine, checked_at: 0, harnesses: [{ ...hv, settings: { ...hv.settings, program: machine === "one" ? savedProgram : "gpu-agent" } }] };
    }
    if (cmd === "harness_settings_save") return new Promise<void>(resolve => { finish = resolve; });
    if (cmd === "harness_settings_all" || cmd === "harness_model_compacts") return [];
    return null;
  }, Channel: class {} }, event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) } } });
  const { act, fireEvent, screen, waitFor, cleanup } = await import("@testing-library/react");
  const { invoke } = await import("../src/ui/tauri.ts");
  const { providersPage } = await import("../src/settings/providers.tsx");
  const view = providersPage({ invoke, openSettings() {}, redrawTop() {}, follow() {} });
  const host = dom.window.document.getElementById("host");
  assert.ok(host);
  try {
    await act(async () => { await view.mount(host, { agent: "codex", settings: true }); });
    await waitFor(() => assert.ok(screen.getByLabelText("Binary path")));
    const input = screen.getByLabelText("Binary path") as HTMLInputElement;
    input.focus();
    fireEvent.change(input, { target: { value: "first" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    fireEvent.change(input, { target: { value: "later" } });
    await act(async () => { finish(); await Promise.resolve(); });
    await waitFor(() => assert.equal((screen.getByLabelText("Binary path") as HTMLInputElement).value, "later"));
    assert.equal(screen.getByLabelText("Binary path"), input);
    assert.equal(dom.window.document.activeElement, input);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    fireEvent.click(screen.getByRole("button", { name: "gpu-box" }));
    await waitFor(() => assert.equal((screen.getByLabelText("Binary path") as HTMLInputElement).value, "gpu-agent"));
    reads.length = 0;
    savedProgram = "later";
    await act(async () => { finish(); await Promise.resolve(); });
    await waitFor(() => assert.ok(reads.includes("one"), "the saved machine must be reread"));
    fireEvent.click(screen.getByRole("button", { name: "build-box" }));
    await waitFor(() => assert.equal((screen.getByLabelText("Binary path") as HTMLInputElement).value, "later"));
  } finally {
    act(() => view.unmount?.()); cleanup(); dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
