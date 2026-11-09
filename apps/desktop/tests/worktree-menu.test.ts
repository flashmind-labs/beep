import { answers, calls } from "./dom-env.ts";
import { JSDOM } from "jsdom";
import test from "node:test";
import assert from "node:assert/strict";
import type { Tree, Entry } from "../src/agentpane/worktree-state.ts";

const dom = new JSDOM('<body><div id="bar" data-mode="bar"></div></body>');
Object.assign(dom.window, { __TAURI__: window.__TAURI__ });
Object.assign(globalThis, { document: dom.window.document, window: dom.window, Element: dom.window.Element, HTMLElement: dom.window.HTMLElement, MutationObserver: dom.window.MutationObserver });
const { showSessionFrame } = await import("../src/agents/session-frame.tsx");
const { startWorktrees, drawWorktrees, followAgent, paneRoot, refreshWorktrees } = await import("../src/agentpane/worktree.ts");
const main: Tree = {path: "/srv/app", branch: "main", head: "abc1234", own: true};
const fix: Tree = {path: "/srv/app/.worktrees/fix", branch: "fix-login", head: "abc1234", own: false};
const tool = (text: string): Entry => ({kind: "tool", text});
const flush = () => new Promise<void>(resolve => setImmediate(resolve));

function setup(id: string, trees: Tree[]) {
  answers.agent_worktrees = trees;
  answers.agent_place = {folder: main.path, main: main.path, linked: false, branch: "main", origin: ""};
  showSessionFrame({ head: null, field: null, body: document.createElement("div"), hints: { value: { items: [], window: false, model: null, link: null }, project: null } }, false, () => {});
  const host = document.createElement("div");
  document.body.append(host);
  const draw = () => drawWorktrees(host, true);
  startWorktrees({id, cwd: main.path, machine_id: "machine", agent: "codex"}, draw);
  return host;
}
function click(host: HTMLElement, selector: string) {
  const element = host.querySelector<HTMLButtonElement>(selector);
  assert.ok(element, selector);
  element.click();
}

test("follow can be disabled on the current checkout and enabled again", async () => {
  const host = setup("toggle", [main, fix]);
  await flush();
  followAgent([tool(`Edit ${fix.path}/file.rs [completed]`)], 0, null);
  await flush();
  assert.equal(paneRoot(), fix.path);
  click(host, ".agwt");
  click(host, '[data-wt="follow"]');
  click(host, ".agwt");
  assert.equal(host.querySelector('[data-wt="follow"]')?.getAttribute("aria-checked"), "false");
  followAgent([tool(`Edit ${main.path}/file.rs [completed]`)], 1, null);
  assert.equal(paneRoot(), fix.path);
  click(host, '[data-wt="follow"]');
  assert.equal(paneRoot(), main.path);
});

test("search filters by branch and path, keeps following available, and can be cleared", async () => {
  const host = setup("search", [main, fix]);
  await flush();
  click(host, ".agwt");
  const input = host.querySelector<HTMLInputElement>('.agwt-search');
  assert.ok(input);
  assert.equal(document.activeElement, input);
  for (const query of ["LOGIN", ".worktrees"]) {
    input.value = query;
    input.dispatchEvent(new dom.window.Event("input", {bubbles: true}));
    assert.equal(host.querySelectorAll('[role="menuitemradio"]:not([hidden])').length, 1);
    assert.equal(host.querySelector('[data-wt="follow"]')?.hasAttribute("hidden"), false);
  }
  input.value = "absent";
  input.dispatchEvent(new dom.window.Event("input", {bubbles: true}));
  assert.equal(host.querySelector('.agwt-empty')?.hasAttribute("hidden"), false);
  input.value = "";
  input.dispatchEvent(new dom.window.Event("input", {bubbles: true}));
  assert.equal(host.querySelectorAll('[role="menuitemradio"]:not([hidden])').length, 2);
});

test("a worktree refresh keeps the search input mounted and focused", async () => {
  const host = setup("refresh", [main, fix]);
  await flush();
  click(host, ".agwt");
  const input = host.querySelector<HTMLInputElement>(".agwt-search");
  assert.ok(input);
  input.value = "fix";
  input.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
  answers.agent_worktrees = [main, fix, { path: "/srv/other", branch: "other", head: "def", own: false }];
  refreshWorktrees();
  await flush();
  assert.equal(host.querySelector(".agwt-search"), input);
  assert.equal(document.activeElement, input);
  assert.equal(input.value, "fix");
});

test("Git list changes reveal candidates, edits confirm usage, and main reads preserve it", async () => {
  setup("detect", [main]);
  await flush();
  const before = calls.filter(call => call.cmd === "agent_worktrees").length;
  answers.agent_worktrees = [main, fix];
  const created = [tool("wrapper [completed]")];
  followAgent(created, 0, null);
  await flush();
  assert.ok(calls.filter(call => call.cmd === "agent_worktrees").length > before);
  assert.equal(paneRoot(), main.path);
  const edited = [...created, tool(`wrapper [completed]\n[diff] .worktrees/fix/file.rs +1 -0\n+text`)];
  followAgent(edited, 0, null);
  await flush();
  assert.equal(paneRoot(), fix.path);
  followAgent([...edited, tool(`Read ${main.path}/README.md [completed]`)], 0, null);
  await flush();
  assert.equal(paneRoot(), fix.path);
  answers.agent_worktrees = [main];
  followAgent([...edited, tool("wrapper [completed]"), tool("wrapper [completed]")], 0, null);
  await flush();
  assert.equal(paneRoot(), main.path);
});
