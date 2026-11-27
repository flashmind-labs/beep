import test from "node:test";
import assert from "node:assert/strict";
import { answers } from "./chat-dom-env.ts";
import type { ScreenRequest } from "../src/parts/center/nav.ts";

// What the sheet reads when it opens.
Object.assign(answers, { settings: { name: "build-box", version: "0.1", autostart: false, agent_notices: false }, agent_hosts: [], list_machines: [], list_credentials: [], list_github_accounts: [] });

const { openScreen, screenWanted } = await import("../src/parts/center/nav.ts");
await import("../src/shell/center/screens.ts");

test("a settings request reaches its followers with its data, and screens.ts opens the sheet", () => {
  const seen: ScreenRequest[] = [];
  screenWanted.follow(request => { seen.push(request); });
  assert.equal(document.querySelector(".cc-set-scrim"), null);
  openScreen({ screen: "settings", section: null });

  assert.deepEqual(seen, [{ screen: "settings", section: null }]);
  assert.ok(document.querySelector(".cc-set-scrim"));
});
