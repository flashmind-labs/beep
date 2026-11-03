import { calls } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { S } from "../src/bar/core.ts";
import { GR, grantClick, grantKeys, type Grant } from "../src/bar/routines.ts";

const grant = (id: string): Grant => ({ id, routine_id: id, routine: id, app: "gmail", action: "Send email", fields: [], per_run: 1, expires_at: 0, created_at: 0, last_used_at: null });
/// A click on row `i` of the grants list.
const clickRow = (i: number) => grantClick({ target: { closest: (sel: string) => sel.startsWith(".drt") ? { dataset: { i: String(i) } } : null } } as unknown as MouseEvent);

test("Enter revokes the grant whose confirm row shows, not one clicked after", async () => {
  S.view = "grants";
  Object.assign(GR, { list: [grant("a"), grant("b")], sel: 0, mode: "confirm", error: "", loaded: true });
  clickRow(1);
  grantKeys({ key: "Enter", preventDefault() {} } as KeyboardEvent, false);
  await new Promise(r => setImmediate(r));
  assert.deepEqual(calls.filter(c => c.cmd === "revoke_grant").map(c => c.args), [{ id: "a" }]);
});
