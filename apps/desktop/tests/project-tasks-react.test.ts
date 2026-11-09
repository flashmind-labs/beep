import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { act, createElement } from "react";
import { ProjectTasks } from "../src/project/tasks-view.tsx";
import type { ProjectView, RowActs } from "../src/project/agent.ts";

test("task and worker controls keep their nodes and act on refreshed data", async () => {
  const dom = new JSDOM("<!doctype html><div id='root'></div>", { url: "https://app.test" });
  const previous = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: dom.window, document: dom.window.document, navigator: dom.window.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    previous.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { value, configurable: true, writable: true });
  }
  const { createRoot } = await import("react-dom/client");
  const host = dom.window.document.getElementById("root");
  assert.ok(host);
  const root = createRoot(host);
  const view: ProjectView = { tasks: [{ id: "task", title: "First", state: "running", depends_on: null, issue_number: null, issue_url: null, branch: null, pr_url: null, worker: "worker", updated_at: 1 }], workers: [{ id: "worker", task_id: "task", machine_id: "machine", machine_name: "Machine", agent: "codex", session: "session", folder: "/srv/project", state: "running", live: true, last_output: null, persona: null, started_at: 1, ended_at: null }], turns: [], cards: [] };
  const changes: string[] = [];
  let busy = false;
  const actions: RowActs = { busy: () => busy, error: () => null, outcome: () => null, openUrl: url => { changes.push(`url:${url}`); }, review: branch => { changes.push(`review:${branch}`); }, merge: task => { changes.push(`merge:${task.id}`); }, diff: branch => () => { changes.push(`diff:${branch}`); }, openWorker: worker => { changes.push(`open:${worker.id}`); }, change: (task, change) => { changes.push(`${task.title}:${change}`); }, stop: worker => { changes.push(`stop:${worker.id}`); } };
  try {
    await act(() => root.render(createElement(ProjectTasks, { view, act: actions })));
    const row = host.querySelector('[data-task="task"]');
    const drop = [...host.querySelectorAll("button")].find(b => b.textContent === "Drop");
    assert.ok(row && drop);
    view.tasks[0] = { ...view.tasks[0], title: "Updated" };
    await act(() => root.render(createElement(ProjectTasks, { view, act: actions })));
    assert.equal(host.querySelector('[data-task="task"]'), row);
    assert.equal(row.querySelector("button"), drop);
    await act(() => drop.click());
    const stop = [...host.querySelectorAll("button")].find(b => b.textContent === "Stop");
    assert.ok(stop);
    await act(() => stop.click());
    assert.deepEqual(changes, ["Updated:drop", "stop:worker"]);
    view.tasks[0] = { ...view.tasks[0], state: "review", branch: "feature", pr_url: "https://example.test/pull/3" };
    await act(() => root.render(createElement(ProjectTasks, { view, act: actions })));
    assert.deepEqual([...row.querySelectorAll("button")].map(b => b.textContent), ["Review", "Merge…", "Open PR", "View diff", "Drop"]);
    for (const button of [...row.querySelectorAll("button")].slice(0, 4)) await act(() => button.click());
    assert.deepEqual(changes.slice(2), ["review:feature", "merge:task", "url:https://example.test/pull/3", "diff:feature"]);
    busy = true;
    await act(() => root.render(createElement(ProjectTasks, { view, act: actions })));
    assert.deepEqual([...row.querySelectorAll("button")].map(b => b.disabled), [false, true, false, false, true]);
    assert.ok(stop.disabled);
    busy = false;
    view.tasks[0] = { ...view.tasks[0], state: "done" };
    await act(() => root.render(createElement(ProjectTasks, { view, act: actions })));
    const reopen = [...row.querySelectorAll("button")].find(b => b.textContent === "Reopen");
    assert.ok(reopen);
    await act(() => reopen.click());
    assert.equal(changes.at(-1), "Updated:reopen");
  } finally {
    await act(() => root.unmount());
    dom.window.close();
    for (const [key, descriptor] of previous) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
});
