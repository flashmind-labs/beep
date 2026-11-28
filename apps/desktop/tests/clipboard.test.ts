import test from "node:test";
import assert from "node:assert/strict";

Object.defineProperty(globalThis, "navigator", { value: { clipboard: { writeText: async () => {} } }, configurable: true });
const { copy } = await import("../src/ui/clipboard.ts");

test("a second click while Copied shows restores the first label", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const button = { innerHTML: "Copy", isConnected: true, set textContent(v: string) { this.innerHTML = v; }, get textContent() { return this.innerHTML; } };
  await copy("x", button as unknown as HTMLElement);
  await copy("x", button as unknown as HTMLElement);
  assert.equal(button.innerHTML, "Copied");
  t.mock.timers.runAll();
  assert.equal(button.innerHTML, "Copy");
});
