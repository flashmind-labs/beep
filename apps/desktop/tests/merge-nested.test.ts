import test from "node:test";
import assert from "node:assert/strict";
import { mergeProjects } from "../src/stores/project/projects-state.ts";

test("a session in a nested project folder counts for the deepest project", () => {
  const at = (name: string, folder: string) => ({ name, url: "", machine_id: "m1", machine_name: "build-box", folder });
  const rows = mergeProjects([at("a", "/srv/mono"), at("b", "/srv/mono/pkg")], [{ machine_id: "m1", cwd: "/srv/mono/pkg/src" } as never]);
  assert.deepEqual(rows.map(r => [r.name, r.agents]), [["a", 0], ["b", 1]]);
});
