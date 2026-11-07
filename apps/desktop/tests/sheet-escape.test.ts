import "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { openSheet, sheetAbove } from "../src/ui/parts.ts";

test("Escape closes the top sheet before Settings and other sheets", () => {
  const listeners: ((event: KeyboardEvent) => void)[] = [];
  document.addEventListener = ((_type: string, fn: EventListener) => { listeners.push(fn as (event: KeyboardEvent) => void); }) as typeof document.addEventListener;
  document.removeEventListener = ((_type: string, fn: EventListener) => {
    const index = listeners.indexOf(fn as (event: KeyboardEvent) => void);
    if (index >= 0) listeners.splice(index, 1);
  }) as typeof document.removeEventListener;
  Object.assign(Node, { DOCUMENT_POSITION_FOLLOWING: 4 });
  const settings = document.createElement("div");
  settings.compareDocumentPosition = () => Node.DOCUMENT_POSITION_FOLLOWING;
  document.body.append(settings);
  const first = openSheet("First", "sheet");
  const second = openSheet("Second", "sheet");
  let settingsClosed = false;
  const escape = () => {
    let stopped = false;
    const event = { key: "Escape", preventDefault() {}, stopImmediatePropagation() { stopped = true; } } as unknown as KeyboardEvent;
    if (!sheetAbove(settings)) settingsClosed = true;
    for (const listener of [...listeners]) { if (!stopped) listener(event); }
  };
  escape();
  assert.equal(settingsClosed, false);
  assert.equal(second.root.isConnected, false);
  assert.equal(first.root.isConnected, true);
  escape();
  assert.equal(first.root.isConnected, false);
  escape();
  assert.equal(settingsClosed, true);
  settings.remove();
});
