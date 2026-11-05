import test from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { localProjectId } from "../src/project/localid.ts";

// A version 5 UUID is the first 16 bytes of the SHA-1 of the namespace and the name.
function reference(name: string) {
  const ns = Buffer.from("6ba7b8119dad11d180b400c04fd430c8", "hex");
  const sum = createHash("sha1").update(ns).update(`beep:project:${name}`).digest().subarray(0, 16);
  sum[6] = (sum[6] & 0x0f) | 0x50;
  sum[8] = (sum[8] & 0x3f) | 0x80;
  const h = sum.toString("hex");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

test("a project's id without a server is the UUID the mesh derives from its name", () => {
  for (const name of ["acme/widget", "a/b", "org/a-very-long-repository-name-that-goes-past-one-hash-block-of-sixty-four-bytes"]) {
    assert.equal(localProjectId(name), reference(name), name);
  }
});
