import test from "node:test";
import assert from "node:assert/strict";

const events = new EventTarget();
const native = new Map<string, () => void>();
let focused = false;
let visibility = "hidden";
Object.assign(globalThis, {
  window: {
    addEventListener: events.addEventListener.bind(events),
    __TAURI__: { event: { listen: async (name: string, fn: () => void) => { native.set(name, fn); return () => {}; } } },
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
