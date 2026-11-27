import { bar } from "./steer-env.ts";
import test from "node:test";
import assert from "node:assert/strict";

const { createElement } = await import("react");
const { flushSync } = await import("react-dom");
const { createRoot } = await import("react-dom/client");
const { createStore } = await import("zustand");
const { CenterWindow, shell } = await import("../src/parts/center/window-shell.tsx");
const { SessionFrame } = await import("../src/parts/session/session-frame-view.tsx");
const { RailList } = await import("../src/screens/project/split-view.tsx");

function spy(label: string, counts: Record<string, number>) {
  function Spy() {
    counts[label] = (counts[label] ?? 0) + 1;
    return createElement("span", { "data-spy": label }, label);
  }
  return createElement(Spy);
}

test("CenterWindow isolates slot updates so sibling parts do not re-render", () => {
  const host = document.createElement("div");
  host.className = "cwin";
  document.body.append(host);
  const root = createRoot(host);
  const counts: Record<string, number> = {};

  shell.setState({
    view: "home",
    assistant: null,
    side: { collapsed: false, rail: null, railHidden: false, over: false },
    parts: {
      title: spy("title", counts),
      sidebar: spy("sidebar", counts),
      home: spy("home", counts),
      notices: spy("notices", counts),
    },
  });
  flushSync(() => root.render(createElement(CenterWindow)));
  assert.deepEqual(counts, { title: 1, sidebar: 1, home: 1, notices: 1 });

  flushSync(() => shell.setState(s => ({ parts: { ...s.parts, notices: spy("notices", counts) } })));
  assert.equal(counts.notices, 2);
  assert.equal(counts.sidebar, 1, "sidebar did not re-render on notices update");
  assert.equal(counts.home, 1, "home did not re-render on notices update");
  assert.equal(counts.title, 1, "title did not re-render on notices update");

  document.body.append(bar);
  root.unmount();
  host.remove();
});

test("SessionFrame isolates head and hints updates from body and field", () => {
  const host = document.createElement("div");
  bar.append(host);
  const root = createRoot(host);
  const counts: Record<string, number> = {};
  const body = Object.assign(document.createElement("div"), { stuck: true, wheel: 0, touching: false });
  const hints = { value: { items: [] as [string, string][], window: false }, project: null };
  const store = createStore(() => ({
    frame: { head: spy("head", counts), field: spy("field", counts), body, hints },
    revision: 1,
    slots: [],
  }));

  flushSync(() => root.render(createElement(SessionFrame, { store })));
  assert.deepEqual(counts, { head: 1, field: 1 });

  flushSync(() => store.setState(s => ({ ...s, frame: { ...s.frame, head: spy("head", counts) } })));
  assert.equal(counts.head, 2);
  assert.equal(counts.field, 1, "field did not re-render on head update");

  flushSync(() => store.setState(s => ({ ...s, frame: { ...s.frame, hints: { value: { items: [["esc", "back"]], window: false }, project: null } } })));
  assert.equal(counts.head, 2, "head did not re-render on hints update");
  assert.equal(counts.field, 1, "field did not re-render on hints update");

  root.unmount();
  host.remove();
});

test("RailList keeps one ResizeObserver across updates", () => {
  const host = document.createElement("div");
  bar.append(host);
  const root = createRoot(host);
  let created = 0, disconnected = 0;
  const PriorObserver = globalThis.ResizeObserver;
  class CountingObserver {
    constructor(_cb: ResizeObserverCallback) { created++; }
    observe() {}
    unobserve() {}
    disconnect() { disconnected++; }
  }
  globalThis.ResizeObserver = CountingObserver as unknown as typeof ResizeObserver;

  try {
    const running = [{
      kind: "agent" as const, machine_id: "m1", machine_name: "box", id: "s1", agent: "codex",
      title: "First", cwd: "/srv/p", status: "working", doing: "step 1", external: false,
      question: null, started_at: 1, last_activity: 1,
    }];
    flushSync(() => root.render(createElement(RailList, { running, past: [], current: null, more: () => {} })));
    assert.equal(created, 1);
    assert.equal(disconnected, 0);

    const updated = [{ ...running[0], doing: "step 2" }];
    flushSync(() => root.render(createElement(RailList, { running: updated, past: [], current: null, more: () => {} })));
    assert.equal(created, 1, "ResizeObserver is not recreated on render");
    assert.equal(disconnected, 0);
  } finally {
    root.unmount();
    host.remove();
    globalThis.ResizeObserver = PriorObserver;
  }
});
