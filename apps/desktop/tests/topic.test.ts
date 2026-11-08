import test from "node:test";
import assert from "node:assert/strict";
import { matchesTopic, normalizeTopic, topic } from "../src/ui/topic-state.ts";

test("dotted topics match exact resources and terminal wildcard scopes", () => {
  assert.equal(matchesTopic("files.org.repo", "files.org.repo"), true);
  assert.equal(matchesTopic("files.org.repo", "files.org.other"), false);
  assert.equal(matchesTopic("files.org.*", "files.org.repo"), true);
  assert.equal(matchesTopic("files.org.*", "files.organization.repo"), false);
  assert.equal(matchesTopic("files.org.*", "files.other.repo"), false);
  assert.equal(matchesTopic("files.org.*", "files.org.repo.branch"), true);
  assert.equal(matchesTopic("files.org.*", "files.org"), true);
  assert.equal(matchesTopic("files.org", "files.org.repo"), false);
  assert.equal(matchesTopic("files.*", "files.org.repo"), true);
  assert.equal(matchesTopic("*", "files.org.repo"), true);
});

test("a broad notification invalidates only overlapping subscription scopes", () => {
  assert.equal(matchesTopic("files.org.repo", "files.*"), true);
  assert.equal(matchesTopic("files.org.repo", "files.org.*"), true);
  assert.equal(matchesTopic("files.org.repo", "files.other.*"), false);
  assert.equal(matchesTopic("files.org.repo", "reviews.*"), false);
  assert.equal(matchesTopic("files.org.repo", "*"), true);
});

test("opaque resource keys and plugin namespaces keep literal dots and wildcard characters", () => {
  assert.equal(topic("files", "/srv/a.b/*%"), "files.%2Fsrv%2Fa%2Eb%2F%2A%25");
  assert.equal(topic("plugin.example", "café"), "plugin%2Eexample.caf%C3%A9");
  assert.equal(matchesTopic(topic("files", "org.repo"), "files.org.repo"), false);
  assert.equal(matchesTopic(topic("files", "*"), "files.org.repo"), false);
  assert.equal(matchesTopic("files.%2a", "files.%2A"), true);
  assert.equal(matchesTopic(topic("plugin.example"), "plugin%2Eexample.resource"), true);
  assert.equal(matchesTopic(topic("plugin.example"), "plugin.example.resource"), false);
});

test("topic normalization canonicalizes escapes and namespace subscriptions", () => {
  assert.equal(normalizeTopic("files"), "files.*");
  assert.equal(normalizeTopic("files.%6f%72%67.repo"), "files.org.repo");
  assert.equal(normalizeTopic("files.a%2eb"), "files.a%2Eb");
  assert.equal(topic("files"), "files.*");
  assert.equal(topic("*"), "*");
});

test("malformed patterns and oversized names are rejected without matching subscriptions", () => {
  for (const value of ["", "files.", "files..repo", "files.*.repo", "*.files", "files.%", "files.%FF", "files.%00", "files.😀", "files." + "a".repeat(4097), "a".repeat(129) + ".*"]) {
    assert.throws(() => normalizeTopic(value));
    assert.equal(matchesTopic("*", value), false);
    assert.equal(matchesTopic(value, "*"), false);
  }
});
