import { test } from "node:test";
import assert from "node:assert/strict";
import { linkified } from "../src/ui/format.ts";

test("web addresses become links and the rest stays text", () => {
  assert.equal(linkified("see https://example.com/a?b=1&c=2, ok"), 'see <a href="https://example.com/a?b=1&amp;c=2">https://example.com/a?b=1&amp;c=2</a>, ok');
  assert.equal(linkified("(http://example.com)."), '(<a href="http://example.com">http://example.com</a>).');
  assert.equal(linkified("<b>no address</b> ftp://x"), "&lt;b&gt;no address&lt;/b&gt; ftp://x");
});
