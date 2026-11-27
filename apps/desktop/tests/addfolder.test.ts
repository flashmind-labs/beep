import test from "node:test";
import assert from "node:assert/strict";
import { folderOffers, noOffer, startOn } from "../src/stores/addproject/addfolder-state.ts";
import { emptyText, unreadText } from "../src/stores/addproject/addproject-state.ts";
import type { MachineFolder } from "../src/stores/project/projects-state.ts";

const folder = (origin: string | null): MachineFolder =>
  ({ path: "/srv/acme/api", parent: "/srv/acme", exists: true, folders: [], more: 0, empty: false, origin, repos: [] });

test("the sheet starts on this computer and only a checkout with an origin offers Add", () => {
  assert.equal(startOn([{ id: "box" }, { id: "mac" }], "mac"), "mac");
  assert.equal(startOn([{ id: "box" }], "mac"), "box");
  assert.equal(startOn([], "mac"), null);
  assert.deepEqual(folderOffers(folder(null)), []);
  assert.equal(noOffer(folder(null)), null);
  assert.deepEqual(folderOffers(folder("")), []);
  assert.equal(noOffer(folder("")), "This checkout has no origin remote. Add one to add it here.");
  assert.deepEqual(folderOffers(folder("git@github.com:acme/api.git")), [{ label: "Add acme/api", path: "/srv/acme/api" }]);
  assert.deepEqual(folderOffers(folder("https://example.test/acme/api.git")), [{ label: "Add acme/api", path: "/srv/acme/api" }]);
});

test("an empty found list names the machines that could not look, not No other folders yet", () => {
  const unread = [unreadText("gpu-box", "still looking for checkouts on this machine")];
  assert.equal(emptyText([], unread, null), "Not read on gpu-box: still looking for checkouts on this machine.");
  assert.equal(emptyText(["build-box"], unread, null), null);
  assert.equal(emptyText([], [], null), "No other folders yet.");
});
