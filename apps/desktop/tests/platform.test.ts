import { test } from "node:test";
import assert from "node:assert/strict";
import { commandKey } from "../src/ui/platform.ts";

test("Windows app shortcuts use Control and macOS uses Command", () => {
  assert.equal(commandKey({ctrlKey: true, metaKey: false}, true), true);
  assert.equal(commandKey({ctrlKey: false, metaKey: true}, true), false);
  assert.equal(commandKey({ctrlKey: true, metaKey: false}, false), false);
  assert.equal(commandKey({ctrlKey: false, metaKey: true}, false), true);
});
