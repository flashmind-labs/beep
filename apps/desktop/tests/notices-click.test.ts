import { answers, calls } from "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import { setImmediate } from "node:timers/promises";
import type { Note } from "../src/center/notices-state.ts";

const routes: { name: string; arg: unknown }[] = [];
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
