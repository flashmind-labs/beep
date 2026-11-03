import { answers, bar } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { S } from "../src/bar/core.ts";
import { renderLink } from "../src/bar/link.ts";

test("the link code running out after leaving the link view leaves the view alone", async t => {
  t.mock.timers.enable({ apis: ["setInterval", "Date"] });
  answers.link_offer = { uri: "beep://link", code: "ABCD", qr_svg: "<svg></svg>", expires_in: 60 };
  await renderLink();
  assert.match(bar.innerHTML, /Waiting for phone/);
  S.view = "settings";
  bar.innerHTML = "settings";
  t.mock.timers.tick(61_000);
  assert.equal(bar.innerHTML, "settings");
});
