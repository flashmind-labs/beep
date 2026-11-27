import test from "node:test";
import assert from "node:assert/strict";
import { mergeProjects, projectName, sideProjects } from "../src/stores/project/projects-state.ts";
import { overviewGroups, searchOverview } from "../src/stores/project/overview-state.ts";

test("renamed projects keep identity, pins and checkout associations while labels and searches update", () => {
  const imported = [{ name: "team/old", display_name: "team/new", url: "https://github.com/team/new", machine_id: "build-box", machine_name: "build-box", folder: "/srv/project" }];
  const rows = mergeProjects(imported, []);
  assert.equal(rows[0].name, "team/old");
  assert.equal(projectName(rows[0]), "team/new");
  assert.deepEqual(rows[0].machines, [{ id: "build-box", name: "build-box", folder: "/srv/project" }]);
  const groups = overviewGroups(rows, ["team/old"]);
  assert.equal(groups[0].cards[0].repo, "new");
  assert.equal(groups[0].cards[0].pinned, true);
  assert.equal(searchOverview(groups, "new").length, 1);
  assert.equal(sideProjects(rows)[1].repo, "new");
  assert.equal(projectName(mergeProjects([{ ...imported[0], display_name: null }], [])[0]), "team/old");
});
