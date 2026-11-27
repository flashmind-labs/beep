import test from "node:test";
import assert from "node:assert/strict";
import { mineLayer } from "../src/stores/settings/instructions-state.ts";

test("saved instructions describe session delivery", () => {
  assert.equal(mineLayer({ version: 3 }), "Version 3, supplied when Beep starts or resumes a session");
  assert.equal(mineLayer(null), "Nothing saved yet, so agents get only the other layers");
});
