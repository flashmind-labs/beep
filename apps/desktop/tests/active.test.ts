import test from "node:test";
import assert from "node:assert/strict";

const events = new EventTarget();
const native = new Map<string, () => void>();
let focused = false;
let visibility = "hidden";
Object.assign(globalThis, {
  window: {
    addEventListener: events.addEventListener.bind(events),
    __TAURI__: { webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async (name: string, fn: () => void) => { native.set(name, fn); return () => {}; } }) } },
  },
  document: {
    hasFocus: () => focused,
    get visibilityState() { return visibility; },
    addEventListener: events.addEventListener.bind(events),
  },
});

const { pageShown, paneShown, onShownChange, setPaneShown, visibilitySnapshot } = await import("../src/ui/active.ts");

test("native show and hide events override stale WebKit visibility and focus", () => {
  const changes: boolean[] = [];
  onShownChange(shown => changes.push(shown));
  assert.equal(pageShown(), false);
  native.get("center-shown")!();
  assert.equal(pageShown(), true, "a shown window can still report hidden and unfocused");
  assert.deepEqual(visibilitySnapshot(), {
    windowHidden: false, nativeHidden: false, paneOff: false, visibility: "hidden", focused: false,
    pageShown: true, paneShown: true, lastEvent: "center-shown",
  });
  focused = true;
  visibility = "visible";
  native.get("center-hidden")!();
  assert.equal(pageShown(), false, "a hidden window can still report visible and focused");
  assert.deepEqual(changes, [true, false]);
});

test("interaction resumes a session even while WebKit reports hidden and unfocused", () => {
  focused = false;
  visibility = "hidden";

  for (const name of ["focus", "keydown", "pointerdown"]) {
    native.get("center-hidden")!();
    events.dispatchEvent(new Event(name));
    assert.equal(pageShown(), true, name);
    assert.equal(visibilitySnapshot().nativeHidden, true, "the last native state stays separate from interaction");
    events.dispatchEvent(new Event("visibilitychange"));
    assert.equal(pageShown(), true, "stale visibility does not undo the interaction");
    assert.equal(visibilitySnapshot().lastEvent, "visibilitychange");
  }
});

test("a shown window only resumes the session when its pane is mounted", () => {
  native.get("center-shown")!();
  const changes: boolean[] = [];
  onShownChange(shown => changes.push(shown), true);
  setPaneShown(false);
  assert.equal(pageShown(), true);
  assert.equal(paneShown(), false);
  setPaneShown(true);
  assert.equal(paneShown(), true);
  assert.deepEqual(changes, [false, true]);
});

test("ambient runs once per wait, runs at once when urgent, and waits while hidden", async t => {
  const { ambient, shownChanged } = await import("../src/ui/active.ts");
  t.mock.timers.enable({ apis: ["setTimeout"] });
  native.get("center-shown")!();
  let showing = true, runs = 0;
  const run = ambient(() => { runs++; }, 1000, () => showing);

  run(); run(); run();
  t.mock.timers.tick(999);
  assert.equal(runs, 0);
  t.mock.timers.tick(1);
  assert.equal(runs, 1, "calls within one wait run once");

  run(true);
  assert.equal(runs, 2, "urgent calls run at once");

  showing = false;
  run(); run();
  t.mock.timers.tick(5000);
  assert.equal(runs, 2, "an off-screen view does not run");
  showing = true;
  shownChanged();
  t.mock.timers.tick(0);
  assert.equal(runs, 3, "it runs once when shown again");

  native.get("center-hidden")!();
  run();
  native.get("center-shown")!();
  t.mock.timers.tick(0);
  assert.equal(runs, 4, "a hidden window catches up when it shows");
});

test("latest gives only the newest read its answer or error", async () => {
  const { latest, STALE } = await import("../src/ui/active.ts");
  const reads = latest();
  let answerOld: (value: string) => void = () => {};
  let failOld: (error: Error) => void = () => {};
  const old = reads.run(new Promise<string>(resolve => { answerOld = resolve; }));
  const newer = reads.run(Promise.resolve("new"));
  answerOld("old");
  assert.equal(await old, STALE);
  assert.equal(await newer, "new");

  const failing = reads.run(new Promise<string>((_, reject) => { failOld = reject; }));
  void reads.run(Promise.resolve("next"));
  failOld(new Error("late"));
  assert.equal(await failing, STALE);
  await assert.rejects(reads.run(Promise.reject(new Error("newest"))), /newest/);

  const cancelled = reads.run(Promise.resolve("kept?"));
  reads.cancel();
  assert.equal(await cancelled, STALE);
});
