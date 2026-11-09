import { JSDOM } from "jsdom";
import { test } from "node:test";
import assert from "node:assert/strict";

const dom = new JSDOM("<body><div id='root'></div></body>");
Object.assign(globalThis, { window: dom.window, document: dom.window.document });
const { createElement } = await import("react");
const { flushSync } = await import("react-dom");
const { createRoot } = await import("react-dom/client");
const { SkillEditor } = await import("../src/lists/skill-editor.tsx");

test("skill validation retains the draft, caret and scroll", () => {
  const host = document.getElementById("root");
  assert.ok(host);
  const root = createRoot(host);
  const skill = { id: "one", name: "Example", description: "When needed", body: "Original", html: "", group_id: null, group_name: null, enabled: true, uses: 0, last_used: null, updated_at: "", edited: false, can_undo: false };
  let saves = 0;
  const render = (error: string) => flushSync(() => root.render(createElement(SkillEditor, { skill, error, hints: "", save: () => saves++, cancel: () => {} })));
  render("");
  const body = host.querySelector<HTMLTextAreaElement>("#kbody");
  assert.ok(body);
  body.value = "New draft";
  body.setSelectionRange(2, 4);
  const scroll = host.querySelector<HTMLElement>(".dscroll");
  assert.ok(scroll);
  scroll.scrollTop = 20;
  render("Give the skill a name.");
  assert.equal(host.querySelector("#kbody"), body);
  assert.equal(body.value, "New draft");
  assert.equal(document.activeElement, body);
  assert.equal(body.selectionStart, 2);
  assert.equal(scroll.scrollTop, 20);
  host.querySelector<HTMLButtonElement>('[data-k="save"]')?.click();
  assert.equal(saves, 1);
  root.unmount();
});
