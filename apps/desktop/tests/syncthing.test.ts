import { test } from "node:test";
import assert from "node:assert/strict";

test("import preserves existing Syncthing rules and ignores stale folder lookups", async () => {
  class Element {
    checked = false;
    indeterminate = false;
    disabled = false;
    children: (Element | string)[] = [];
    events = new Map<string, () => void>();
    append(...children: (Element | string)[]) { this.children.push(...children); }
    addEventListener(name: string, on: () => void) { this.events.set(name, on); }
  }
  const reads: { args: Record<string, unknown>; reply: (files: unknown) => void }[] = [];
  Object.defineProperty(globalThis, "window", { configurable: true, value: {
    __TAURI__: { core: { Channel: class {}, invoke: (cmd: string, args: Record<string, unknown>) =>
      cmd === "log_webview" || cmd === "desktop_trace_enabled" ? Promise.resolve(null) : new Promise(reply => reads.push({ args, reply })) }, event: { listen: async () => () => {} } },
    addEventListener: () => {},
  } });
  Object.defineProperty(globalThis, "document", { configurable: true, value: {
    createElement: () => new Element(), addEventListener: () => {},
  } });
  const { syncthingImportChoice } = await import("../src/project/syncthing.ts");
  const choice = syncthingImportChoice();
  const checkbox = ((choice.node as unknown as Element).children[0] as Element).children[0] as Element;
  assert.equal(choice.value(), null);
  const first = choice.follow("build-box", "/srv/projects/one");
  const second = choice.follow("build-box", "/srv/projects/two");
  reads[1].reply({ available: true, enabled: false, label: "Projects" });
  await second;
  reads[0].reply({ available: true, enabled: true, label: "Projects" });
  await first;
  assert.equal(checkbox.checked, false);
  assert.equal(choice.value(), null, "reading the setting must not change it during import");
  assert.ok(reads.every(read => read.args.enabled === undefined));
  checkbox.checked = true;
  checkbox.events.get("change")!();
  assert.equal(choice.value(), true);
  await choice.follow("build-box", "/srv/projects/three");
  assert.equal(reads.length, 2, "a chosen import preference must survive folder changes");
});
