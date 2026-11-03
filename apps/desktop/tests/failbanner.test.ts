import "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { splitFailure } from "../src/components/failbanner.ts";

const failed = "Could not update OpenCode on build-box: opencode upgrade failed: Error: Unexpected EOF (tried brew upgrade opencode, then opencode upgrade)\n\n$ /srv/brew/bin/brew upgrade opencode\nError: untrusted tap\n\n$ /srv/brew/bin/opencode upgrade\nError: Unexpected EOF";

test("a failure splits into its line and the output behind it", () => {
  const { line, details } = splitFailure(failed);
  assert.equal(line, "Could not update OpenCode on build-box: opencode upgrade failed: Error: Unexpected EOF (tried brew upgrade opencode, then opencode upgrade)");
  assert.ok(details.startsWith("$ /srv/brew/bin/brew upgrade opencode\nError: untrusted tap\n\n$ "));
  assert.deepEqual(splitFailure("one\nline"), { line: "one line", details: "" });
});
