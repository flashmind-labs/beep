import { answers, bar, calls } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { editorTools, openInEditor, type EditorRow } from "../src/project/editors.ts";
import { drawEditors } from "../src/agents/session.ts";

const settle = () => new Promise<void>(resolve => setImmediate(resolve));

test("editor discovery retries after failure and redraws every waiting header", async t => {
  t.mock.method(console, "warn", () => {});
  answers.editors = Promise.reject(new Error("Unavailable"));
  assert.deepEqual(editorTools("/srv/project", () => assert.fail("failed discovery redrew")), []);
  await settle();

  let resolve!: (rows: EditorRow[]) => void;
  answers.editors = new Promise<EditorRow[]>(done => { resolve = done; });
  const headers: string[] = [];
  const first = () => headers.push("first");
  editorTools("/srv/project", first);
  editorTools("/srv/project", first);
  editorTools("/srv/other", () => headers.push("second"));
  resolve([{ id: "vscode", name: "VS Code" }]);
  await settle();
  assert.deepEqual(headers, ["first", "second"]);
  const buttons = editorTools("/srv/project", () => {});
  assert.equal(buttons.length, 1);
  assert.equal(buttons[0].textContent, "Open in VS Code");
});

test("a late folder lookup cannot overwrite the current session's editor button", async () => {
  bar.innerHTML = '<span id="agedit"></span>';
  let resolve!: (folder: string) => void;
  answers.session_folder_here = new Promise<string>(done => { resolve = done; });
  const previous = drawEditors("m1", "/srv/previous", () => true);
  answers.session_folder_here = "/srv/current";
  await drawEditors("m1", "/srv/current", () => true);
  const main = bar.querySelector("#agedit button")!;
  assert.equal(main.textContent, "Open in VS Code");
  assert.equal(Reflect.get(main, "title"), "/srv/current");
  resolve("/srv/previous");
  await previous;
  assert.equal(Reflect.get(bar.querySelector("#agedit button")!, "title"), "/srv/current");
  const reads = calls.filter(c => c.cmd === "session_folder_here").length;
  await drawEditors("m1", "/srv/current", () => true);
  assert.equal(calls.filter(c => c.cmd === "session_folder_here").length, reads);
});

test("a replacement header gets editor buttons even for the same folder", async () => {
  bar.innerHTML = '<span id="agedit"></span>';
  answers.session_folder_here = "/srv/current";
  await drawEditors("m1", "/srv/current", () => true);
  assert.equal(bar.querySelector("#agedit button")!.textContent, "Open in VS Code");
});

test("editor launch failures are visible from keyboard and menu actions", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  answers.editors = [{ id: "zed", name: "Zed" }];
  answers.open_in_editor = Promise.reject(new Error("Cannot launch application"));

  await openInEditor("/srv/project");

  assert.match(bar.querySelector("#saved")?.textContent ?? "", /Could not open Zed:.*Cannot launch application/);
  assert.equal(bar.querySelector("#saved")?.getAttribute("role"), "status");
  assert.deepEqual(calls.filter(c => c.cmd === "open_in_editor").at(-1)?.args, {
    editor: "zed", path: "/srv/project",
  });
  bar.querySelector("#saved")?.remove();
  answers.open_in_editor = null;
});

test("a successful editor launch shows no error", async () => {
  answers.editors = [{ id: "zed", name: "Zed" }];
  answers.open_in_editor = null;

  await openInEditor("/srv/project");

  assert.equal(bar.querySelector("#saved"), null);
});
