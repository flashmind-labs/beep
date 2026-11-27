import test from "node:test";
import assert from "node:assert/strict";
import { createAssetCache, parseAssetRef, type AssetBackend } from "../src/stores/plugins/assets.ts";
import { createSpriteDriver, createSpriteScript, type SpriteHost, type SpriteMotion } from "../src/stores/plugins/sprite.ts";

const ref = (digit: string) => ({ hash: digit.repeat(64), mime: "image/png" });

/// A backend that answers loads when the test says so, and records what it was asked.
function fakeBackend() {
  const loads: { hash: string; resolve: (bytes: ArrayBuffer) => void; reject: () => void }[] = [];
  const revoked: string[] = [];
  let made = 0;
  const backend: AssetBackend = {
    load: hash => new Promise((resolve, reject) => { loads.push({ hash, resolve, reject: () => reject(new Error("no image")) }); }),
    create: () => `blob:${++made}`,
    revoke: url => { revoked.push(url); },
  };
  return { backend, loads, revoked };
}

const settle = () => new Promise<void>(resolve => setImmediate(resolve));

test("asset references need a hex hash and an image type", () => {
  assert.deepEqual(parseAssetRef(ref("a")), ref("a"));
  for (const bad of [null, "a", { hash: "a", mime: "image/png" }, { hash: "A".repeat(64), mime: "image/png" }, { hash: "a".repeat(64), mime: "text/html" }, { hash: "a".repeat(64) }]) {
    assert.equal(parseAssetRef(bad), null);
  }
});

test("an image used by many placements is fetched once", async () => {
  const fake = fakeBackend();
  let changes = 0;
  const cache = createAssetCache(fake.backend, () => { changes++; });

  cache.use([ref("a"), ref("a"), ref("b")]);
  cache.use([ref("a"), ref("b")]);
  assert.deepEqual(fake.loads.map(load => load.hash[0]), ["a", "b"]);

  fake.loads[0].resolve(new ArrayBuffer(1));
  await settle();
  assert.deepEqual([...cache.urls()], [[ref("a").hash, "blob:1"]]);
  assert.equal(changes, 1);

  // Using it again later asks for nothing.
  cache.use([ref("a"), ref("b")]);
  assert.equal(fake.loads.length, 2);
});

test("an image nobody uses has its blob URL revoked, and is fetched again if it comes back", async () => {
  const fake = fakeBackend();
  const cache = createAssetCache(fake.backend, () => {});
  cache.use([ref("a"), ref("b")]);
  fake.loads[0].resolve(new ArrayBuffer(1));
  fake.loads[1].resolve(new ArrayBuffer(1));
  await settle();

  cache.use([ref("b")]);
  assert.deepEqual(fake.revoked, ["blob:1"]);
  assert.deepEqual([...cache.urls().keys()], [ref("b").hash]);

  cache.use([]);
  assert.deepEqual(fake.revoked, ["blob:1", "blob:2"]);
  assert.equal(cache.urls().size, 0);

  cache.use([ref("a")]);
  assert.equal(fake.loads.length, 3);
});

test("an image that goes out of use while loading never gets a URL", async () => {
  const fake = fakeBackend();
  const cache = createAssetCache(fake.backend, () => {});
  cache.use([ref("a")]);
  cache.use([]);

  fake.loads[0].resolve(new ArrayBuffer(1));
  await settle();

  assert.equal(cache.urls().size, 0);
  assert.deepEqual(fake.revoked, []);
});

test("a failed fetch is not retried while the image stays in use", async () => {
  const fake = fakeBackend();
  const cache = createAssetCache(fake.backend, () => {});
  cache.use([ref("a")]);
  fake.loads[0].reject();
  await settle();

  cache.use([ref("a")]);
  assert.equal(fake.loads.length, 1);
  cache.use([]);
  cache.use([ref("a")]);
  assert.equal(fake.loads.length, 2);
});

/// Random numbers that come from a list, then 0.
const sequence = (...values: number[]) => { let at = 0; return () => values[at++] ?? 0; };

const every = (minMs: number, maxMs: number) => ({ kind: "every", minMs, maxMs }) as const;
const blink: SpriteMotion = { steps: [{ frame: 1, ms: 140 }], repeat: every(3000, 7000) };
const tail: SpriteMotion = { steps: [{ frame: 2, ms: 200 }, { frame: 3, ms: 200 }], repeat: every(5000, 11000) };

test("a repeating motion waits a random delay, plays, rests, and waits again", () => {
  const script = createSpriteScript([blink], 0, sequence(0.5, 0, 1));

  // 3000 + 0.5 * 4000 = 5000 ms to the first play.
  assert.deepEqual(script.begin(0), { frame: 0, wait: 5000 });
  assert.deepEqual(script.next(5000), { frame: 1, wait: 140 });
  // The delay is counted from the end of the play: 3000 ms with random 0.
  assert.deepEqual(script.next(5140), { frame: 0, wait: 3000 });
  assert.deepEqual(script.next(8140), { frame: 1, wait: 140 });
  // Random just under 1 gives the longest delay.
  assert.deepEqual(script.next(8280), { frame: 0, wait: 7000 });
});

test("motions play one at a time, and a due motion waits its turn", () => {
  const script = createSpriteScript([blink, tail], 0, sequence(0, 0));

  // Blink is due at 3000 and tail at 5000.
  assert.deepEqual(script.begin(0), { frame: 0, wait: 3000 });
  assert.deepEqual(script.next(3000), { frame: 1, wait: 140 });
  assert.deepEqual(script.next(3140), { frame: 0, wait: 1860 });
  assert.deepEqual(script.next(5000), { frame: 2, wait: 200 });
  assert.deepEqual(script.next(5200), { frame: 3, wait: 200 });
  // Blink's next play is due at 3140 + 3000 = 6140.
  assert.deepEqual(script.next(5400), { frame: 0, wait: 740 });
});

test("motions that fell due while the page waited play in due order, one after the other", () => {
  const script = createSpriteScript([blink, tail], 0, () => 0);
  script.begin(0);
  script.next(3000);
  // Blink ends at 3140 and is next due at 6140. Tail was due at 5000, so it goes first.
  script.next(3140);
  assert.deepEqual(script.next(9000), { frame: 2, wait: 200 });
  assert.deepEqual(script.next(9200), { frame: 3, wait: 200 });
  assert.deepEqual(script.next(9400), { frame: 1, wait: 140 });
});

test("a once motion plays when the sprite appears, then the rest frame holds with no timer", () => {
  const once: SpriteMotion = { steps: [{ frame: 1, ms: 100 }, { frame: 2, ms: 100 }], repeat: { kind: "once" } };
  const script = createSpriteScript([once], 0, () => 0);

  assert.deepEqual(script.begin(10), { frame: 1, wait: 100 });
  assert.deepEqual(script.next(110), { frame: 2, wait: 100 });
  assert.deepEqual(script.next(210), { frame: 0, wait: null });
});

test("a loop never rests", () => {
  const loop: SpriteMotion = { steps: [{ frame: 1, ms: 80 }, { frame: 2, ms: 80 }], repeat: { kind: "loop" } };
  const script = createSpriteScript([loop], 0, () => 0);

  assert.deepEqual(script.begin(0), { frame: 1, wait: 80 });
  assert.deepEqual(script.next(80), { frame: 2, wait: 80 });
  assert.deepEqual(script.next(160), { frame: 1, wait: 80 });
});

test("a sprite with no motions rests with no timer", () => {
  assert.deepEqual(createSpriteScript([], 2, () => 0).begin(0), { frame: 2, wait: null });
});

/// A host with a clock and timers the test advances by hand.
function fakeHost() {
  let clock = 0;
  let nextId = 1;
  const timers = new Map<number, { at: number; fn: () => void }>();
  const frames: number[] = [];
  const host: SpriteHost = {
    now: () => clock,
    setFrame: frame => { frames.push(frame); },
    setTimer: (fn, ms) => { const id = nextId++; timers.set(id, { at: clock + ms, fn }); return id; },
    clearTimer: id => { timers.delete(id); },
  };
  /// Moves the clock to the next timer and runs it. Returns false when none is pending.
  const step = () => {
    const next = [...timers.entries()].sort((a, b) => a[1].at - b[1].at)[0];
    if (next === undefined) return false;
    timers.delete(next[0]);
    clock = next[1].at;
    next[1].fn();
    return true;
  };
  return { host, frames, timers, step };
}

test("the driver shows frames from timers, one pending at a time, and rests when made inactive", () => {
  const fake = fakeHost();
  const driver = createSpriteDriver(createSpriteScript([blink], 0, () => 0), 0, fake.host);

  driver.setActive(true);
  assert.deepEqual(fake.frames, [0]);
  assert.equal(fake.timers.size, 1);

  fake.step();
  fake.step();
  assert.deepEqual(fake.frames, [0, 1, 0]);
  assert.equal(fake.timers.size, 1);

  // Hiding holds the rest frame and leaves no timer behind.
  fake.step();
  driver.setActive(false);
  assert.equal(fake.frames.at(-1), 0);
  assert.equal(fake.timers.size, 0);
  assert.equal(fake.step(), false);
});

test("a driver that is inactive from the start never sets a timer, and stop clears the one pending", () => {
  const fake = fakeHost();
  const driver = createSpriteDriver(createSpriteScript([blink], 0, () => 0), 0, fake.host);

  driver.setActive(false);
  assert.equal(fake.timers.size, 0);
  assert.deepEqual(fake.frames, []);

  driver.setActive(true);
  driver.stop();
  assert.equal(fake.timers.size, 0);
});

test("a driver starts over when it becomes active again", () => {
  const fake = fakeHost();
  const driver = createSpriteDriver(createSpriteScript([blink], 0, () => 0), 0, fake.host);
  driver.setActive(true);
  fake.step();
  driver.setActive(false);

  driver.setActive(true);

  assert.equal(fake.timers.size, 1);
  assert.equal(fake.frames.at(-1), 0);
});

const loop: SpriteMotion = { steps: [{ frame: 1, ms: 80 }, { frame: 2, ms: 80 }], repeat: { kind: "loop" } };

test("a reaction plays once over the motions, then they start over", () => {
  const fake = fakeHost();
  const driver = createSpriteDriver(createSpriteScript([loop], 0, () => 0), 0, fake.host);
  driver.setActive(true);
  fake.step();
  assert.deepEqual(fake.frames, [1, 2]);

  driver.play([{ frame: 3, ms: 100 }, { frame: 4, ms: 50 }]);
  assert.equal(fake.timers.size, 1);
  fake.step();
  fake.step();
  assert.deepEqual(fake.frames, [1, 2, 3, 4, 1]);

  // The loop goes on, and the reaction does not come back.
  fake.step();
  fake.step();
  assert.deepEqual(fake.frames, [1, 2, 3, 4, 1, 2, 1]);
  assert.equal(fake.timers.size, 1);
});

test("a reaction needs an active driver, and hiding the sprite drops it", () => {
  const fake = fakeHost();
  const driver = createSpriteDriver(createSpriteScript([blink], 0, () => 0), 0, fake.host);

  driver.play([{ frame: 3, ms: 100 }]);
  assert.deepEqual(fake.frames, []);
  assert.equal(fake.timers.size, 0);

  driver.setActive(true);
  driver.play([{ frame: 3, ms: 100 }, { frame: 4, ms: 100 }]);
  driver.setActive(false);
  assert.equal(fake.frames.at(-1), 0);
  assert.equal(fake.timers.size, 0);

  // Shown again, the motions begin, not the half-played reaction.
  driver.setActive(true);
  assert.equal(fake.frames.at(-1), 0);
  fake.step();
  assert.equal(fake.frames.at(-1), 1);
});

test("a second reaction replaces the one playing", () => {
  const fake = fakeHost();
  const driver = createSpriteDriver(createSpriteScript([], 0, () => 0), 0, fake.host);
  driver.setActive(true);

  driver.play([{ frame: 3, ms: 100 }, { frame: 4, ms: 100 }]);
  driver.play([{ frame: 5, ms: 40 }]);
  assert.equal(fake.timers.size, 1);
  fake.step();

  assert.deepEqual(fake.frames, [0, 3, 5, 0]);
  assert.equal(fake.timers.size, 0);
});
