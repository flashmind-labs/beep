import test from "node:test";
import assert from "node:assert/strict";
import type { AgentSession } from "../src/agents/agents-state.ts";

Object.assign(globalThis, {
  window: {
    addEventListener: () => {},
    __TAURI__: {
      core: { invoke: async () => [], Channel: class {} },
      event: { listen: async () => () => {} },
      webviewWindow: { getCurrentWebviewWindow: () => ({ listen: async () => () => {} }) },
    },
  },
  document: { addEventListener: () => {}, visibilityState: "visible" },
});

const { createPrSessionLookup } = await import("../src/project/pr-session.ts");

const session = (id: string): AgentSession => ({
  id, agent: "claude", machine_id: "m1", machine_name: "build-box",
  cwd: "/srv/project", title: "PR session", status: "ended", doing: null,
  started_at: 1, last_activity: 1, external: false, question: null,
  assistant: false, persona: null,
});

const deferred = <T,>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(r => { resolve = r; });
  return { promise, resolve };
};

const flushMicrotasks = () => new Promise<void>(resolve => setTimeout(resolve, 0));

test("prSession coalesces concurrent completions and stays linear across sequential redraws", async () => {
  const pending = new Map<string, ReturnType<typeof deferred<AgentSession | null>>>();
  const prSession = createPrSessionLookup(url => {
    const next = deferred<AgentSession | null>();
    pending.set(url, next);
    return next.promise;
  });
  const urls = Array.from({ length: 20 }, (_, i) => `https://example.test/org/repo/pull/${i + 1}`);

  let draws = 0;
  const redraw = () => {
    draws++;
    for (const url of urls) prSession(url, redraw);
  };

  for (const url of urls) assert.equal(prSession(url, redraw), "checking");
  assert.equal(pending.size, 20);

  // Resolve the first ten together: one coalesced redraw.
  for (let i = 0; i < 10; i++) pending.get(urls[i])!.resolve(null);
  await flushMicrotasks();
  assert.equal(draws, 1);

  // Resolve the remaining ten one at a time, redrawing after each: linear, never 2^N.
  for (let i = 10; i < 20; i++) {
    pending.get(urls[i])!.resolve(null);
    await flushMicrotasks();
  }
  assert.equal(draws, 11);
  for (const url of urls) assert.equal(prSession(url, redraw), null);
});

test("prSession keeps found sessions cached and retries misses after the retry window", async () => {
  let clock = 1_000;
  let calls = 0;
  let answer: AgentSession | null = null;
  const prSession = createPrSessionLookup(async () => { calls++; return answer; }, () => clock);
  const redraw = () => {};

  // A miss caches for the retry window, then retries.
  assert.equal(prSession("https://example.test/org/repo/pull/1", redraw), "checking");
  await flushMicrotasks();
  assert.equal(calls, 1);
  assert.equal(prSession("https://example.test/org/repo/pull/1", redraw), null);
  assert.equal(calls, 1);

  clock += 60_000;
  answer = session("s1");
  assert.equal(prSession("https://example.test/org/repo/pull/1", redraw), "checking");
  await flushMicrotasks();
  assert.equal(calls, 2);
  const found = prSession("https://example.test/org/repo/pull/1", redraw);
  assert.ok(found && found !== "checking");
  assert.equal(found.id, "s1");

  // Once found, the session stays cached past the retry window.
  clock += 120_000;
  const cached = prSession("https://example.test/org/repo/pull/1", redraw);
  assert.ok(cached && cached !== "checking");
  assert.equal(cached.id, "s1");
  assert.equal(calls, 2);
});
