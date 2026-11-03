import test from "node:test";
import assert from "node:assert/strict";
import { liveText } from "../src/karaoke.ts";

test("a thought streams as plain text without Markdown marks", () => {
  assert.equal(liveText("\n\n**Checking 391**\n\n\n\nIt is `17 x 23`."), "Checking 391\n\nIt is 17 x 23.");
  assert.equal(liveText("## Plan\nRead __it__"), "Plan\nRead it");
});
