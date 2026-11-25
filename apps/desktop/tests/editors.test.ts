import { answers, bar, calls } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { editorsFor, type EditorRow } from "../src/project/editors.ts";
import { drawEditors } from "../src/agents/session-head.tsx";
import "../src/bar/bar-app.tsx";
import { DraftHead, draftMessage, showSessionFrame, shownMessage } from "../src/agents/session-frame.tsx";
import { createElement } from "react";
import { createStore } from "zustand/vanilla";

const settle = () => new Promise<void>(resolve => setImmediate(resolve));

test("editor discovery retries after failure and redraws every waiting header", async t => {
  t.mock.method(console, "warn", () => {});
  answers.editors = Promise.reject(new Error("Unavailable"));
  assert.deepEqual(editorsFor("/srv/project", () => assert.fail("failed discovery redrew")), []);
  await settle();

  let resolve!: (rows: EditorRow[]) => void;
  answers.editors = new Promise<EditorRow[]>(done => { resolve = done; });
  const headers: string[] = [];
  const first = () => headers.push("first");
  editorsFor("/srv/project", first);
  editorsFor("/srv/project", first);
  editorsFor("/srv/other", () => headers.push("second"));
  resolve([{ id: "vscode", name: "VS Code" }]);
  await settle();
  assert.deepEqual(headers, ["first", "second"]);
  const editors = editorsFor("/srv/project", () => {});
  assert.deepEqual(editors, [{ id: "vscode", name: "VS Code" }]);
});

test("a late folder lookup cannot overwrite the current session's editor button", async () => {
  showSessionFrame({ head: createElement(DraftHead, { project: "project", openProject() {}, machine: createStore<string>(() => "") }), field: null, body: document.createElement("div"), hints: { value: { items: [], window: false }, project: null } }, false, () => {});
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
  const button = bar.querySelector<HTMLButtonElement>("#agedit button"); assert.ok(button); button.focus();
  await drawEditors("m1", "/srv/current", () => true);
  assert.equal(calls.filter(c => c.cmd === "session_folder_here").length, reads);
  assert.equal(document.activeElement, button);
});

test("a replacement header gets editor buttons even for the same folder", async () => {
  showSessionFrame({ head: createElement(DraftHead, { project: "project", openProject() {}, machine: createStore<string>(() => "") }), field: null, body: document.createElement("div"), hints: { value: { items: [], window: false }, project: null } }, false, () => {});
  answers.session_folder_here = "/srv/current";
  await drawEditors("m1", "/srv/current", () => true);
  assert.equal(bar.querySelector("#agedit button")!.textContent, "Open in VS Code");
});

test("a session drawn in place of a draft releases the draft's field", () => {
  const head = createElement(DraftHead, { project: "project", openProject() {}, machine: createStore<string>(() => "") });
  const hints = { value: { items: [], window: false, model: null, link: null }, project: null };
  const field = document.createElement("textarea");
  showSessionFrame({ head, field: null, body: document.createElement("div"), hints }, false, () => { draftMessage.current = null; });
  draftMessage.current = field;
  document.body.append(field);
  assert.equal(shownMessage(), field);

  let released = 0;
  showSessionFrame({ head, field: null, body: document.createElement("div"), hints }, true, () => { released++; });
  assert.equal(draftMessage.current, null);
  assert.equal(released, 0);
  field.remove();
  draftMessage.current = field;
  assert.equal(shownMessage(), null, "a detached field does not take the keys");
  draftMessage.current = null;
});
