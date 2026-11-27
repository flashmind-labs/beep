import { test } from "node:test";
import assert from "node:assert/strict";

test("batches desktop timings without sending command arguments", async () => {
  const calls: { cmd: string; args?: Record<string, unknown> }[] = [];
  const events = new Map<string, () => void>();
  Object.defineProperty(globalThis, "window", { configurable: true, value: {
    __TAURI__: {
      core: {
        Channel: class {},
        invoke: async (cmd: string, args?: Record<string, unknown>) => { calls.push({ cmd, args }); return cmd === "build_capabilities" ? { local_host: true, metrics_recording: true, autostart: true, image_clipboard: true, desktop_control: true, option_taps: true } : null; },
      },
      event: { listen: async () => () => {} }, webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) },
    },
    addEventListener: (name: string, cb: () => void) => events.set(name, cb),
  } });
  Object.defineProperty(globalThis, "document", { configurable: true, value: {
    hidden: false, addEventListener: () => {},
  } });
  const { buildCapabilities, invoke, recordTiming } = await import("../src/ui/tauri.ts");
  await buildCapabilities();
  recordTiming("page", "agent", 42);
  await invoke("agent_view", { session: "private" });
  events.get("pagehide")!();
  const batch = calls.find(call => call.cmd === "desktop_timings")?.args?.samples as { kind: string; name: string }[];
  assert.deepEqual(batch.map(({ kind, name }) => [kind, name]), [["page", "agent"], ["command", "agent_view"]]);
  assert.ok(!JSON.stringify(batch).includes("private"));
});
