import "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { openSheet, sheetAbove } from "../src/ui/parts.ts";

test("Escape closes the top sheet before Settings and other sheets", () => {
  const settings = document.createElement("div");
  document.body.append(settings);
  let settingsClosed = false;
  const settingsKeys = (event: KeyboardEvent) => {
    if (event.key === "Escape" && !sheetAbove(settings)) settingsClosed = true;
  };
  document.addEventListener("keydown", settingsKeys, true);
  const first = openSheet("First", "sheet");
  const second = openSheet("Second", "sheet");
  const escape = () => document.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  escape();
  assert.equal(settingsClosed, false);
  assert.equal(second.root.isConnected, false);
  assert.equal(first.root.isConnected, true);
  escape();
  assert.equal(first.root.isConnected, false);
  escape();
  assert.equal(settingsClosed, true);
  document.removeEventListener("keydown", settingsKeys, true);
  settings.remove();
});


test("a sheet mounts and closes in its supplied host without disturbing siblings", () => {
  const host = document.createElement("div"), sibling = document.createElement("button");
  host.append(sibling);
  document.body.append(host);
  sibling.focus();
  let closed = 0;
  const sheet = openSheet("Hosted dialog", "sheet", { host, onClose: () => { closed++; } });
  assert.equal(sheet.root.parentElement, host);
  sheet.close();
  sheet.close();
  assert.equal(sheet.root.isConnected, false);
  assert.equal(closed, 1);
  assert.equal(host.firstElementChild, sibling);
  assert.equal(document.activeElement, sibling);
  host.remove();
});
