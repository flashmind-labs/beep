import { answers, bar } from "./bar-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { S } from "../src/bar/core.ts";
import { loginFailed, renderLink } from "../src/bar/link.ts";

test("the link code running out after leaving the link view leaves the view alone", async t => {
  t.mock.timers.enable({ apis: ["setInterval", "Date"] });
  answers.login_hosted = false;
  answers.link_offer = { uri: "beep://link", code: "ABCD", qr_svg: "<svg></svg>", expires_in: 60 };
  await renderLink();
  assert.match(bar.innerHTML, /Waiting for phone/);
  S.view = "settings";
  bar.innerHTML = "settings";
  t.mock.timers.tick(61_000);
  assert.equal(bar.innerHTML, "settings");
});

test("a hosted server asks to sign in, and a bad token says so", async () => {
  answers.login_hosted = true;
  await renderLink();
  assert.equal(S.view, "login");
  assert.match(bar.innerHTML, /Sign in to Beep/);
  assert.match(bar.innerHTML, /Use my own server/);
  assert.doesNotMatch(bar.innerHTML, /Waiting for phone/);
  loginFailed("bad request: that sign-in expired or was for another device");
  assert.match(bar.innerHTML, /That sign-in expired or was for another device\./);
  assert.match(bar.innerHTML, /Try again/);
  loginFailed("the server did not answer in 15 seconds");
  assert.match(bar.innerHTML, /the server did not answer/);
});
