import { JSDOM } from "jsdom";
import { test } from "node:test";
import assert from "node:assert/strict";

const dom = new JSDOM("<body><div id='root'></div></body>");
Object.assign(globalThis, { window: dom.window, document: dom.window.document });
const { createElement } = await import("react");
const { flushSync } = await import("react-dom");
const { createRoot } = await import("react-dom/client");
const { ThreadCardContent } = await import("../src/center/threadcard-view.tsx");

test("thread refresh keeps the post mounted and expands earlier comments", () => {
  const host = document.getElementById("root"); assert.ok(host);
  const root = createRoot(host);
  const view = { kind: "issue", number: 1, title: "Issue", url: "https://example.test", author: "Author", body: "Original post", needs_reply: false, earlier: 1, comments: Array.from({ length: 4 }, (_, i) => ({ author: `User ${i}`, at: 1000 + i, body: `Comment ${i}` })) };
  let all = false;
  const draw = () => flushSync(() => root.render(createElement(ThreadCardContent, { view, label: "Issue 1", where: "On its host", error: "", all, expand: () => { all = true; draw(); } })));
  draw();
  const post = host.querySelector(".tx"); assert.ok(post);
  assert.equal(host.querySelectorAll(".c").length, 4);
  const retained = [...host.querySelectorAll(".tx")].find(node => node.textContent === "Comment 2");
  assert.ok(retained);
  view.comments.push({ author: "Another", at: 2000, body: "New comment" });
  draw();
  assert.equal(host.querySelector(".tx"), post);
  assert.equal([...host.querySelectorAll(".tx")].find(node => node.textContent === "Comment 2"), retained);
  const more = host.querySelector<HTMLButtonElement>("button"); assert.ok(more); more.click();
  assert.equal(host.querySelector(".tx"), post);
  assert.equal(host.querySelectorAll(".c").length, 6);
  assert.match(host.textContent ?? "", /1 older comment on the host/);
  root.unmount();
});
