import { calls, answers } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { chooseWith, folderFacts, nextFolder, startOn } from "../src/addproject/addfolder-state.ts";
import { emptyText, unreadText } from "../src/addproject/addproject-state.ts";
import type { MachineFolder } from "../src/project/projects-state.ts";

// The fake DOM drops listeners; keep each element's, so a test can click.
type Clickable = { on?: Record<string, (e: unknown) => void> };
(globalThis as unknown as { HTMLElement: { prototype: Record<string, unknown> } }).HTMLElement.prototype.addEventListener =
  function (this: Clickable, type: string, fn: (e: unknown) => void) { (this.on ||= {})[type] = fn; };
const click = (el: unknown) => (el as Clickable).on!.click({});
const { openAddFolder } = await import("../src/addproject/addfolder.ts");

const folder = (origin: string | null): MachineFolder =>
  ({ path: "/srv/acme/api", parent: "/srv/acme", exists: true, folders: [], more: 0, empty: false, origin });

test("Choose a folder opens the sheet only: the macOS picker waits for Choose… on this Mac", () => {
  const machines = [{ id: "mac", name: "MacBook Air", online: true }, { id: "box", name: "build-box", online: true }];
  openAddFolder({ machines, macId: "mac" });
  assert.ok(!calls.some(c => c.cmd === "pick_folder"), "opening asks for no folder");
  const sheet = document.querySelector(".ip-sheet")!;
  click(sheet.querySelector(".af-choose"));
  assert.equal(calls.filter(c => c.cmd === "pick_folder").length, 1);

  calls.length = 0;
  answers.list_machine_folder = folder(null);
  click(sheet.querySelectorAll(".dchip")[1]);
  click(sheet.querySelector(".af-choose"));
  assert.ok(!calls.some(c => c.cmd === "pick_folder"), "another machine browses");
  assert.deepEqual(calls.map(c => c.cmd), ["list_machine_folder"]);
});

test("the sheet starts on this Mac and a checkout with an origin is what Add project takes", () => {
  const start = startOn([{ id: "box", online: true }, { id: "mac", online: true }], "mac");
  assert.equal(start.machine, "mac");
  assert.equal(chooseWith("mac", "mac"), "picker");
  assert.equal(chooseWith("box", "mac"), "browser");
  const plain = nextFolder(start, { type: "choose", listing: folder(null) });
  assert.equal(nextFolder(plain, { type: "submit" }), plain);
  assert.deepEqual(folderFacts(plain.chosen, plain.branch), ["Not a git checkout. Choose the folder that holds .git."]);
  const checkout = nextFolder(start, { type: "choose", listing: folder("git@github.com:acme/api.git") });
  assert.equal(checkout.branch, undefined);
  const read = nextFolder(checkout, { type: "branch", path: "/srv/acme/api", branch: "main" });
  assert.deepEqual(folderFacts(read.chosen, read.branch), ["A checkout of acme/api, added in place.", "Remote git@github.com:acme/api.git · Branch main"]);
  assert.equal(nextFolder(read, { type: "submit" }).step, "adding");
});

test("an empty found list names the machines that could not look, not No other folders yet", () => {
  const unread = [unreadText("gpu-box", "still looking for checkouts on this machine")];
  assert.equal(emptyText([], unread, null), "Not read on gpu-box: still looking for checkouts on this machine.");
  assert.equal(emptyText(["build-box"], unread, null), null);
  assert.equal(emptyText([], [], null), "No other folders yet.");
});
