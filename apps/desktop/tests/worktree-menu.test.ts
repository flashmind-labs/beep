import { answers, calls } from "./dom-env.ts";
import { JSDOM } from "jsdom";
import test from "node:test";
import assert from "node:assert/strict";
import type { Tree, Entry } from "../src/agentpane/worktree-state.ts";

const dom = new JSDOM('<body><div id="bar" data-mode="bar"></div></body>');
Object.assign(dom.window, { __TAURI__: window.__TAURI__ });
Object.assign(globalThis, { document: dom.window.document, window: dom.window, Element: dom.window.Element, HTMLElement: dom.window.HTMLElement, MutationObserver: dom.window.MutationObserver });
const { createElement } = await import("react");
const { flushSync } = await import("react-dom");
const { createRoot } = await import("react-dom/client");
const { WorktreeMenu } = await import("../src/agentpane/worktree-view.tsx");
const { takePlaces } = await import("../src/agents/place.ts");
const { startWorktrees, worktreeMenu, followAgent, paneRoot, takeWorktrees } = await import("../src/agentpane/worktree.ts");
const main: Tree = {path: "/srv/app", branch: "main", head: "abc1234", own: true, created: false};
const fix: Tree = {path: "/srv/app/.worktrees/fix", branch: "fix-login", head: "abc1234", own: false, created: true};
const tool = (text: string): Entry => ({kind: "tool", text});
const flush = () => new Promise<void>(resolve => setImmediate(resolve));

function setup(id: string, trees: Tree[]) {
  answers.agent_worktrees = trees;
  answers.agent_places = {kind: "found", places: [{folder: main.path, main: main.path, linked: false, branch: "main", origin: ""}]};
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const draw = () => {
    const menu = worktreeMenu();
    flushSync(() => root.render(menu && createElement(WorktreeMenu, menu)));
  };
  startWorktrees({id, cwd: main.path, machine_id: "machine", agent: "codex"}, draw);
  return host;
}
/// A `files` update of the session's folder, carrying its worktree list.
const push = (session: string, value: Tree[]) => takeWorktrees({ views: [{ command: "agent_worktrees", args: { machineId: "machine", folder: main.path, session }, value }] });
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
  input.value = "fixes";
  input.dispatchEvent(new dom.window.Event("input", { bubbles: true }));
  input.setSelectionRange(3, 3);
  push("refresh", [main, fix, { path: "/srv/other", branch: "other", head: "def", own: false, created: true }]);
  assert.equal(host.querySelector(".agwt-search"), input);
  assert.equal(document.activeElement, input);
  assert.equal(input.value, "fixes");
  assert.equal(input.selectionStart, 3, "the caret stays where the user left it");
});

test("Git list changes reveal candidates, edits confirm usage, and main reads preserve it", async () => {
  setup("detect", [main]);
  await flush();
  const before = calls.filter(call => call.cmd === "agent_worktrees").length;
  const created = [tool("wrapper [completed]")];
  push("detect", [main, fix]);
  followAgent(created, 0, null);
  await flush();
  assert.equal(paneRoot(), main.path);
  const edited = [...created, tool(`wrapper [completed]\n[diff] .worktrees/fix/file.rs +1 -0\n+text`)];
  followAgent(edited, 0, null);
  await flush();
  assert.equal(paneRoot(), fix.path);
  followAgent([...edited, tool(`Read ${main.path}/README.md [completed]`)], 0, null);
  await flush();
  assert.equal(paneRoot(), fix.path);
  push("detect", [main]);
  followAgent([...edited, tool("wrapper [completed]"), tool("wrapper [completed]")], 0, null);
  await flush();
  assert.equal(paneRoot(), main.path);
  assert.equal(calls.filter(call => call.cmd === "agent_worktrees").length, before, "tool calls finishing never read the list");
});

test("Flashmind's active worktree appears in the picker after Git refreshes the list", async () => {
  const host = setup("flashmind", [main]);
  await flush();
  push("flashmind", [main, { ...fix, created: false }]);
  followAgent([
    tool(`worktree: Worktree create [completed]\n${JSON.stringify({ path: fix.path, current: true, available: true, bare: false })}`),
    tool("exec: cargo check [completed]"),
  ], 0, null);
  await flush();
  assert.equal(paneRoot(), fix.path);
  assert.equal(host.querySelector(".agwt code")?.textContent, fix.branch);
  click(host, ".agwt");
  assert.match(host.querySelector('[aria-checked="true"][role="menuitemradio"]')?.textContent ?? "", /\.worktrees\/fix.*agent here/);
});

test("the menu lists the worktrees the session made, and every one with Show all", async () => {
  const other: Tree = {path: "/srv/app/.worktrees/other", branch: "other", head: "abc1234", own: false, created: false};
  const host = setup("made", [main, fix, other]);
  await flush();
  const asked = calls.filter(call => call.cmd === "agent_worktrees").at(-1)?.args;
  assert.ok(asked && typeof asked === "object" && "session" in asked);
  assert.equal(asked.session, "made");
  click(host, ".agwt");
  const listed = () => [...host.querySelectorAll('[role="menuitemradio"]')].map(item => item.getAttribute("data-wt"));
  assert.deepEqual(listed(), [main.path, fix.path]);
  const all = host.querySelector<HTMLInputElement>(".agwt-all input");
  assert.ok(all);
  all.click();
  assert.deepEqual(listed(), [main.path, fix.path, other.path]);
  all.click();
  assert.deepEqual(listed(), [main.path, fix.path]);
});


test("native cwd reports follow relative shell edits into a worktree and back", async t => {
  t.mock.timers.enable({apis: ["setTimeout"]});
  setup("native-cwd", [main, fix]);
  answers.agent_places = {kind: "found", places: [{folder: fix.path, main: main.path, linked: true, branch: "fix-login", origin: ""}]};
  await flush();
  followAgent([tool("python3 edit.py [completed]")], 0, null);
  await flush();
  assert.equal(paneRoot(), fix.path);

  // The session's folder tells where it works now.
  takePlaces({ views: [{ command: "agent_places", args: { machineId: "machine", asks: [{ agent: "codex", session: "native-cwd", external: false }] }, value: { kind: "found", places: [{folder: main.path, main: main.path, linked: false, branch: "main", origin: ""}] } }] });
  t.mock.timers.tick(150);
  followAgent([tool("python3 edit.py [completed]")], 0, null);
  await flush();
  assert.equal(paneRoot(), main.path);
  followAgent([tool("python3 edit.py [completed]"), tool("python3 next.py [completed]")], 0, null);
  await flush();
  assert.equal(paneRoot(), main.path);
});
