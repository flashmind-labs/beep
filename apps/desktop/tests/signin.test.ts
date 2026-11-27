import test from "node:test";
import assert from "node:assert/strict";
import { readSignIn, signInOffer, stillRunning, type SignInView } from "../src/stores/machines/signin-state.ts";

test("another machine offers Sign in for the agents it signs in itself", () => {
  for (const tool of ["claude", "codex", "opencode", "agy"]) assert.deepEqual(signInOffer(tool, "build-box", true, false), { button: true });
  // Not while it is offline, and never on this computer.
  assert.equal(signInOffer("claude", "build-box", false, false), null);
  assert.equal(signInOffer("claude", "this-mac", true, true), null);
});

test("an agent with no sign-in link says how to sign in there", () => {
  assert.deepEqual(signInOffer("pi", "build-box", false, false), { how: "Run pi on build-box and use /login" });
  assert.equal(signInOffer("unknown-tool", "build-box", true, false), null);
});

test("the sheet asks again only while the sign-in runs", () => {
  const view = (state: SignInView["state"]): SignInView => ({ state, url: null, code: null, asks_code: false, reason: null });
  assert.ok(stillRunning(view("starting")));
  assert.ok(stillRunning(view("waiting")));
  assert.ok(!stillRunning(view("signed_in")));
  assert.ok(!stillRunning(view("failed")));
  assert.ok(!stillRunning(view("idle")));
});

test("a sign-in update carries the state the sheet draws", () => {
  assert.deepEqual(readSignIn({ SignIn: { Waiting: { url: "https://example.test/device", code: "AB-12", asks_code: false } } }),
    { state: "waiting", url: "https://example.test/device", code: "AB-12", asks_code: false, reason: null });
  assert.equal(readSignIn({ SignIn: { Failed: { reason: "timed out" } } })?.reason, "timed out");
  assert.equal(readSignIn({ SignIn: { SignedIn: {} } })?.state, "signed_in");
  assert.equal(readSignIn({ SignIn: { Idle: {} } })?.state, "idle");
  assert.equal(readSignIn({ error: "refused" }), null);
  assert.equal(readSignIn(undefined), null);
});
