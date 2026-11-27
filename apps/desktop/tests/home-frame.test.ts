import "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";

Object.assign(globalThis, { HTMLInputElement: window.HTMLInputElement });
const { frameOf } = await import("../src/stores/center/home-frame-state.ts");

test("a frame is an inert copy without the pane, media and typed drafts", () => {
  const win = document.createElement("div");
  win.className = "cwin";
  win.style.setProperty("--side-w", "240px");
  win.innerHTML = `<aside id="side">Projects</aside><div id="pane-box"><div id="bar">chat</div></div>
    <iframe src="about:blank"></iframe><textarea id="draft">old draft</textarea><input id="ask" value="old">`;
  const draft = win.querySelector<HTMLTextAreaElement>("textarea");
  const ask = win.querySelector<HTMLInputElement>("input");
  assert.ok(draft && ask);
  draft.value = "a typed draft";
  ask.value = "typed too";

  const { skin, html } = frameOf(win, "linear");
  const holder = document.createElement("div");
  holder.innerHTML = html;
  const copy = holder.firstElementChild;
  assert.ok(copy);

  assert.equal(skin, "linear");
  assert.ok(copy.classList.contains("cwin") && copy.classList.contains("kept"));
  assert.ok(copy.hasAttribute("inert"));
  assert.equal(copy.getAttribute("aria-hidden"), "true");
  assert.equal(copy.getAttribute("style"), "--side-w: 240px;");
  assert.equal(copy.querySelector("#side")?.textContent, "Projects");
  assert.equal(copy.querySelector("#pane-box")?.children.length, 0);
  assert.equal(copy.querySelector("iframe"), null);
  assert.equal(copy.querySelector("textarea")?.textContent, "");
  assert.equal(copy.querySelector("input")?.getAttribute("value"), null);
  assert.ok(!html.includes("old draft") && !html.includes("typed"));

  assert.equal(win.className, "cwin");
  assert.ok(!win.hasAttribute("inert"));
  assert.ok(win.querySelector("iframe"));
  assert.equal(draft.value, "a typed draft");
});
