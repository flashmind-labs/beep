import test from "node:test";
import assert from "node:assert/strict";
import { ACTION_LABEL, linkChip, serverKind, serverView, SIGN_OUT_ASK, signedOutFooter } from "../src/settings/server-state.ts";
import { firstRunDue } from "../src/settings/subscription.ts";
import { pageLabel } from "../src/settings/settings-nav.ts";

test("the section is called Beep server", () => {
  assert.equal(pageLabel("devices"), "Beep server");
});

test("a signed out Mac is signed out whatever else is known", () => {
  for (const link of ["unlinked", "linked", "revoked"] as const) {
    for (const reach of [null, "none", "answered", "unreachable"] as const) assert.equal(serverKind(link, true, reach), "signed-out");
  }
});

test("a linked Mac is signed in unless the server did not answer", () => {
  assert.equal(serverKind("linked", false, null), "signed-in");
  assert.equal(serverKind("linked", false, "answered"), "signed-in");
  assert.equal(serverKind("linked", false, "unreachable"), "unreachable");
});

test("a Mac with no account, or removed from it, has nothing to sign out of", () => {
  assert.equal(serverKind("unlinked", false, null), "no-account");
  assert.equal(serverKind("revoked", false, "answered"), "no-account");
});

test("signed in offers Sign out and lists the devices", () => {
  const v = serverView("signed-in");
  assert.deepEqual(v.actions, ["sign-out"]);
  assert.equal(v.devices, true);
  assert.equal(v.dot, "ok");
  assert.equal(v.title, "Signed in");
});

test("not reachable offers Try again and Sign out, with no devices to list", () => {
  const v = serverView("unreachable");
  assert.deepEqual(v.actions, ["retry", "sign-out"]);
  assert.equal(v.devices, false);
  assert.equal(v.dot, "bad");
});

test("signed out offers only Sign in, and says what keeps working", () => {
  const v = serverView("signed-out");
  assert.deepEqual(v.actions, ["sign-in"]);
  assert.equal(v.devices, false);
  assert.equal(v.dot, "off");
  assert.match(v.note, /Machines, agents and projects keep working/);
});

test("no account offers Sign in and says what it adds", () => {
  const v = serverView("no-account");
  assert.deepEqual(v.actions, ["sign-in"]);
  assert.match(v.note, /memories/);
});

test("the section's words are plain: no em-dash and no arrows", () => {
  const words = [SIGN_OUT_ASK.title, SIGN_OUT_ASK.note, ...Object.values(ACTION_LABEL)];
  for (const kind of ["signed-in", "unreachable", "signed-out", "no-account"] as const) words.push(serverView(kind).note);
  for (const w of words) assert.doesNotMatch(w, /[—→⇒←]/, w);
});

test("signing out is confirmed in one line that says what stops", () => {
  assert.equal(SIGN_OUT_ASK.note.split(". ").length, 2);
  assert.match(SIGN_OUT_ASK.note, /assistant and voice go off/);
});

test("the bar says Signed out where it says No subscription for a Mac with no account", () => {
  assert.deepEqual(linkChip("unlinked", true), ["off", "Signed out"]);
  assert.deepEqual(linkChip("unlinked", false), ["off", "No subscription"]);
  assert.deepEqual(linkChip("revoked", false), ["bad", "Removed"]);
  assert.equal(linkChip("linked", false), null);
});

test("the sidebar footer leads to Sign in only while signed out", () => {
  assert.deepEqual(signedOutFooter(true), { text: "Signed out", action: "Sign in" });
  assert.equal(signedOutFooter(false), null);
});

test("a Mac signed out never gets the first-run window", () => {
  assert.equal(firstRunDue(false, null, true), false);
  assert.equal(firstRunDue(false, null, false), true);
});
