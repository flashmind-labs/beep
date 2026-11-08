import { answers, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import { setImmediate } from "node:timers/promises";
import type { Note } from "../src/center/notices-state.ts";

const routes: { name: string; arg: unknown }[] = [];
let updateNotes: (event: { payload: Note[] }) => void = () => {};
window.__TAURI__.event.listen = async (name, handler) => {
  if (name === "notes") updateNotes = handler as typeof updateNotes;
  return () => {};
};
Object.assign(globalThis, { noticeRoutes: routes });
const stubs = new Map(Object.entries({
  "center/views.ts": "export const go = (name, arg) => globalThis.noticeRoutes.push({ name, arg });",
  "bar/pane.ts": "export const pane = {};",
  "settings/sheet.ts": "export const openSettings = () => {};",
}).map(([path, source]) => [new URL(`../src/${path}`, import.meta.url).href, source]));
const hooks = registerHooks({
  load(url, context, next) {
    const source = stubs.get(url);
    return source === undefined ? next(url, context) : { format: "module", source, shortCircuit: true };
  },
});
const { mountNotices } = await import("../src/center/notices.ts");
hooks.deregister();

for (const [result, read, activation] of [
  ["passed", true, "click"],
  ["failed", false, "Enter"],
  ["passed", false, " "],
] as const) {
  test(`opening a ${result} build with ${JSON.stringify(activation)} removes its notification`, async t => {
    const listeners = new WeakMap<HTMLElement, Map<string, EventListener>>();
    t.mock.method(HTMLElement.prototype, "addEventListener", function (this: HTMLElement, name: string, handler: EventListener) {
      let events = listeners.get(this);
      if (!events) listeners.set(this, events = new Map());
      events.set(name, handler);
    });
    const fire = (el: HTMLElement, name: string, key?: string) => {
      const handler = listeners.get(el)?.get(name);
      assert.ok(handler, `${name} handler exists`);
      handler.call(el, { target: el, key, preventDefault() {} } as unknown as Event);
    };
    const target = { projects: { project: "acme/widget", run: { id: 7, host: "github.com" } } };
    const build: Note = {
      id: "build:7", kind: "run", title: `Build ${result} on main`, text: "CI #7",
      source: "acme/widget", at: 2, read, done: false, target,
    };
    answers.alerts_list = null;
    answers.notes_list = [build, { ...build, id: "other", kind: "finished", title: "Agent finished", at: 1, target: null }];
    calls.length = 0;
    routes.length = 0;
    const host = document.createElement("div");
    mountNotices(host);
    await setImmediate();
    fire(host.querySelector<HTMLElement>(".nt-bell")!, "click");
    const row = host.querySelector<HTMLElement>(".nt-row")!;
    assert.match(row.textContent!, /Build/);
    fire(row, activation === "click" ? "click" : "keydown", activation);

    assert.deepEqual(routes, [{ name: "projects", arg: target.projects }]);
    assert.equal(host.querySelector(".nt-pop"), null, "opening the build closes the pane");
    fire(host.querySelector<HTMLElement>(".nt-bell")!, "click");
    assert.doesNotMatch(host.textContent!, /Build/);
    assert.match(host.textContent!, /Agent finished/, "unrelated notifications stay");
    assert.deepEqual(calls.filter(c => c.cmd === "notes_dismiss"), [
      { cmd: "notes_dismiss", args: { ids: [build.id] } },
    ], "removal is saved by the backend");
    await setImmediate();
  });
}

test("Cmd+B focuses notifications; arrows, updates and dismissals keep keyboard focus in the list", async t => {
  const listeners = new WeakMap<object, Map<string, EventListener>>();
  const listen = function (this: object, name: string, handler: EventListener) {
    let events = listeners.get(this);
    if (!events) listeners.set(this, events = new Map());
    events.set(name, handler);
  };
  t.mock.method(HTMLElement.prototype, "addEventListener", listen);
  t.mock.method(document, "addEventListener", listen);
  let active: HTMLElement = document.body;
  Object.defineProperty(document, "activeElement", { configurable: true, get: () => active });
  t.after(() => { Reflect.deleteProperty(document, "activeElement"); });
  t.mock.method(HTMLElement.prototype, "focus", function (this: HTMLElement) { active = this; });
  const fire = (target: HTMLElement, name: string, keys: Partial<KeyboardEvent> = {}) => {
    const path: object[] = [];
    for (let node: HTMLElement | null = target; node; node = node.parentElement) path.push(node);
    path.push(document);
    let stopped = false;
    const event = {
      target, defaultPrevented: false, ...keys,
      preventDefault() { this.defaultPrevented = true; },
      stopPropagation() { stopped = true; },
      composedPath: () => path,
    };
    for (const node of path) {
      listeners.get(node)?.get(name)?.call(node, event as unknown as Event);
      if (stopped) break;
    }
    return event;
  };
  const key = (key: string, metaKey = false) => fire(active!, "keydown", { key, metaKey });
  const note = (id: string, at: number): Note => ({
    id, at, kind: "finished", title: id, text: "", source: "build-box", read: false, done: false, target: null,
  });
  const notes = [note("first", 3), note("second", 2), note("third", 1)];
  answers.alerts_list = null;
  answers.notes_list = notes;
  const host = document.createElement("div");
  const opener = document.createElement("button");
  document.body.append(host, opener);
  t.after(() => { host.remove(); opener.remove(); });
  mountNotices(host);
  await setImmediate();

  opener.focus();
  assert.equal(key("b", true).defaultPrevented, true);
  assert.equal(active?.dataset.id, "first", "the shortcut focuses the first notification");
  key("ArrowUp");
  assert.equal(active?.dataset.id, "third", "Up wraps to the last notification");
  key("ArrowDown");
  assert.equal(active?.dataset.id, "first", "Down wraps to the first notification");
  key("ArrowDown");
  assert.equal(active?.dataset.id, "second");

  updateNotes({ payload: [note("new", 4), ...notes] });
  assert.equal(host.querySelectorAll(".nt-row").length, 4);
  assert.equal(active?.dataset.id, "second", "an incoming notification preserves the focused row");
  active!.querySelector<HTMLElement>("button")!.focus();
  key("ArrowDown");
  assert.equal(active?.dataset.id, "third", "arrows also work from a row action");
  active!.querySelector<HTMLElement>("button")!.focus();
  fire(active!, "click");
  assert.equal(active?.closest<HTMLElement>(".nt-row")?.dataset.id, "second", "dismissing the last row focuses its neighbor");

  assert.equal(key("Escape").defaultPrevented, true, "Escape does not navigate the page behind the pane");
  assert.equal(host.querySelector(".nt-pop"), null);
  assert.equal(active, opener, "closing restores the previous focus");
  key("b", true);
  key("b", true);
  assert.equal(active, opener, "the shortcut also restores focus when closing");

  updateNotes({ payload: [] });
  key("b", true);
  assert.equal(active, host.querySelector(".nt-pop"), "an empty panel still receives focus");
  updateNotes({ payload: notes });
  assert.equal(active?.dataset.id, "first", "notes loaded after opening receive focus");
  updateNotes({ payload: [{ ...notes[0], kind: "skill_conflict" }] });
  key("Enter");
  assert.equal(host.querySelector(".nt-row"), null, "Enter opens the notification");
  key("Escape");
  assert.equal(active?.dataset.id, "first", "Escape returns to the opened notification");
  key("Escape");
  assert.equal(active, opener);
});
