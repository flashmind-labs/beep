import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { forgetView, readView, viewKey, writeView } from "../src/stores/agentpane/view-memory.ts";

test("project views survive reconstruction and closing removes only that session", () => {
  const dom = new JSDOM("", { url: "https://example.test" });
  Object.defineProperty(globalThis, "localStorage", { value: dom.window.localStorage, configurable: true });
  const key = viewKey("machine", "session");
  const other = viewKey("machine", "session-other");
  const state = { on: { right: "changes", bottom: "text:sample.ts" }, tabs: [{ root: "/srv/project", rel: "sample.ts" }] };
  writeView(key, state);
  writeView(key + ":diff", { scroll: 1200 });
  writeView(other, state);
  assert.deepEqual(readView(key), state);
  assert.deepEqual(readView(key + ":diff"), { scroll: 1200 });
  forgetView("machine", "session");
  assert.equal(readView(key), null);
  assert.equal(readView(key + ":diff"), null);
  assert.deepEqual(readView(other), state);
  writeView(key, state);
  assert.equal(readView(key), null, "a closing pane cannot recreate its saved state");
  dom.window.close();
});
