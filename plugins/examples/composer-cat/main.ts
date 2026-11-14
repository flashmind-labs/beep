/// <reference path="../../sdk/beep.d.ts" />

// A cat sits on the message box. Now and then it blinks, flicks its tail or
// twitches an ear, one motion at a time. The page plays these from a
// declaration, so the plugin sleeps between events. When the turn of the
// session whose message box is open ends, it is happy for a moment.

import { isRecord, readTurn, type Turn } from "./turn.ts";

/** The session whose message box is open, as `ui.composer` says. */
interface Composer {
  readonly machine: string;
  readonly session: string;
}

const WIDTH = 36;
const HEIGHT = 28;
const SCALE = 2;
const HAPPY_MS = 1400;

const art = (name: string): string => `art/cat-${name}.png`;

/**
 * The idle motions are declared, and the page plays them: each starts after a
 * random delay in its range, one at a time, and the cat rests on its idle
 * frame in between. Nothing here wakes the plugin.
 */
const MOTIONS: readonly SpriteMotion[] = [
  {
    steps: [{ frame: art("blink"), ms: 140 }],
    repeat: { every: { min: 3000, max: 7000 } },
  },
  {
    steps: [
      { frame: art("tail-a"), ms: 200 },
      { frame: art("tail-b"), ms: 200 },
      { frame: art("tail-a"), ms: 200 },
      { frame: art("tail-b"), ms: 200 },
      { frame: art("tail-a"), ms: 200 },
    ],
    repeat: { every: { min: 5000, max: 11000 } },
  },
  {
    steps: [
      { frame: art("ear"), ms: 220 },
      { frame: art("idle"), ms: 120 },
      { frame: art("ear"), ms: 220 },
    ],
    repeat: { every: { min: 6000, max: 13000 } },
  },
];

const happy = beep.state(false);
let stopHappy: (() => void) | null = null;

/** Show the happy frame for a moment. The one timer here ends it. */
function cheer(): void {
  stopHappy?.();
  happy.set(true);
  stopHappy = beep.after(HAPPY_MS, () => {
    stopHappy = null;
    happy.set(false);
  });
}

/** `{ machine, session }` from the `ui.composer` topic, or null when it is anything else. */
function readComposer(data: unknown): Composer | null {
  if (!isRecord(data)) {
    return null;
  }
  const { machine, session } = data;
  if (typeof machine !== "string" || typeof session !== "string") {
    return null;
  }

  return { machine, session };
}

let followed: Composer | null = null;
let unfollow: (() => void) | null = null;

/** Follow the turn of the composer's session, and drop the previous one. */
function follow(composer: Composer | null): void {
  if (composer?.machine === followed?.machine && composer?.session === followed?.session) {
    return;
  }

  unfollow?.();
  unfollow = null;
  followed = composer;
  if (composer === null) {
    return;
  }

  let before: Turn | null = null;
  unfollow = beep.on(
    { name: "agent", key: composer.session, machine: composer.machine },
    (event) => {
      const turn = readTurn(event.data, composer.session);
      if (turn === null) {
        return;
      }
      if (before === "running" && turn === "done") {
        cheer();
      }
      before = turn;
    },
  );
}

beep.on({ name: "ui.composer" }, (event) => follow(readComposer(event.data)));

beep.place({ slot: "composer", align: "end", offset: 84, sink: 3 }, () =>
  happy.get()
    ? beep.image({
        src: art("happy"),
        width: WIDTH * SCALE,
        height: HEIGHT * SCALE,
        pixelated: true,
      })
    : beep.sprite({
        rest: art("idle"),
        width: WIDTH * SCALE,
        height: HEIGHT * SCALE,
        pixelated: true,
        motions: MOTIONS,
      }),
);
