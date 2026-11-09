import { answers, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { P } from "../src/agentpane/media.ts";
import { filesView } from "../src/agentpane/files.ts";
import { startWorktrees } from "../src/agentpane/worktree.ts";

const settle = () => new Promise(resolve => setImmediate(resolve));

test("a bulk context menu preserves selection and Cmd+Delete waits for confirmation on the shown root", async t => {
  t.mock.timers.enable({ apis: ["setInterval", "setTimeout"] });
  const listeners = new WeakMap<object, Map<string, ((e: unknown) => unknown)[]>>();
  const proto = Object.getPrototypeOf(document.createElement("div"));
  Object.defineProperty(proto, "attributes", { configurable: true, get() {
    return [...this.attrs as Map<string, string>].map(([name, value]) => ({ name, value }));
  } });
  t.mock.method(proto, "addEventListener", function(this: object, kind: string, fn: (e: unknown) => unknown) {
    let map = listeners.get(this);
    if (!map) { map = new Map(); listeners.set(this, map); }
    map.set(kind, [...map.get(kind) ?? [], fn]);
  });
  Object.assign(document, { removeEventListener() {} });
  const send = async (el: Element, kind: string, extra: Record<string, unknown> = {}) => {
    const event = { target: el, preventDefault() {}, stopPropagation() {}, ...extra };
    for (const fn of listeners.get(el)?.get(kind) ?? []) await fn(event);
    await settle();
  };
  P.session = { id: "files-test", machine_id: "m1", machine_name: "build-box", cwd: "/srv/project", agent: "codex" };
  answers.agent_worktrees = [];
  startWorktrees(P.session, () => {});
  answers.agent_tree = { folders: [{ path: "", entries: ["a.txt", "b.txt", "report.html"].map(name => ({ name, folder: false, size: 1, ignored: false, change: null })), more: 0, git: true }], truncated: false };
  const view = filesView();
  document.body.append(view);
  await settle();
  const tree = view.querySelector(".agtree")!;
  const row = (name: string) => tree.querySelector(`[data-fpath="${name}"]`)!;
  await send(tree, "contextmenu", { target: row("report.html"), clientX: 0, clientY: 0 });
  assert.ok(view.querySelector('[data-fact="preview"]'), "HTML files offer preview");
  await send(tree, "click", { target: tree });
  await send(tree, "contextmenu", { target: row("a.txt"), clientX: 0, clientY: 0 });
  assert.equal(view.querySelector('[data-fact="preview"]'), null, "text files do not offer preview");
  await send(tree, "click", { target: tree });
  await send(tree, "click", { target: row("a.txt"), metaKey: true });
  await send(tree, "click", { target: row("b.txt"), metaKey: true });
  await send(tree, "contextmenu", { target: row("a.txt"), clientX: 0, clientY: 0 });
  assert.equal(tree.querySelectorAll('[aria-selected="true"]').length, 2);
  assert.match(view.querySelector(".agctx")!.textContent!, /Delete 2 items permanently/);
  calls.length = 0;
  await send(tree, "keydown", { key: "Backspace", metaKey: true });
  assert.equal(calls.filter(c => c.cmd === "agent_tree_edit").length, 0);
  const dialog = document.querySelector('[role="dialog"]')!;
  assert.match(dialog.textContent!, /cannot be undone/);
  const act = [...dialog.querySelectorAll("button")].find(b => b.textContent === "Delete 2 items")!;
  await send(act, "click");
  const edit = calls.find(c => c.cmd === "agent_tree_edit");
  assert.deepEqual(edit?.args, { machineId: "m1", root: "/srv/project", action: { Delete: { paths: ["a.txt", "b.txt"] } } });
  assert.ok(calls.some(c => c.cmd === "agent_tree" && (c.args as { fresh: boolean }).fresh));
  view.remove();
});
