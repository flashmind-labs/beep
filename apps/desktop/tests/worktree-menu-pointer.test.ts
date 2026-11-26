import { answers } from "./dom-env.ts";
import { JSDOM } from "jsdom";
import test from "node:test";
import assert from "node:assert/strict";
import type { Tree } from "../src/agentpane/worktree-state.ts";

const dom = new JSDOM('<body><div id="bar" data-mode="bar"></div></body>');
Object.assign(dom.window, { __TAURI__: window.__TAURI__ });
Object.assign(globalThis, { document: dom.window.document, window: dom.window, Element: dom.window.Element, HTMLElement: dom.window.HTMLElement, MutationObserver: dom.window.MutationObserver });
Object.assign(dom.window.HTMLElement.prototype, { checkVisibility: () => true });
const { createElement } = await import("react");
const { flushSync } = await import("react-dom");
const { createRoot } = await import("react-dom/client");
const { WorktreeMenu } = await import("../src/agentpane/worktree-view.tsx");
const { startWorktrees, worktreeMenu } = await import("../src/agentpane/worktree.ts");
const main: Tree = {path: "/srv/app", branch: "main", head: "abc1234", own: true, created: false};
const fix: Tree = {path: "/srv/app/.worktrees/fix", branch: "fix-login", head: "abc1234", own: false, created: true};
const flush = () => new Promise<void>(resolve => setImmediate(resolve));

test("a pointerdown whose target is not an element closes the open menu", async () => {
  answers.agent_worktrees = [main, fix];
  answers.agent_places = {kind: "found", places: [{folder: main.path, main: main.path, linked: false, branch: "main", origin: ""}]};
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const draw = () => {
    const menu = worktreeMenu();
    flushSync(() => root.render(menu && createElement(WorktreeMenu, menu)));
  };
  startWorktrees({id: "pointer", cwd: main.path, machine_id: "machine", agent: "codex"}, draw, () => false);
  await flush();
  host.querySelector<HTMLButtonElement>(".agwt")?.click();
  assert.equal(worktreeMenu()?.open, true);
  dom.window.dispatchEvent(new dom.window.Event("pointerdown"));
  assert.equal(worktreeMenu()?.open, false);
});
