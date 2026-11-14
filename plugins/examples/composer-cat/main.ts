/// <reference path="../../sdk/beep.d.ts" />

// A cat sits on the message box. Now and then it blinks, flicks its tail or
// twitches an ear, one motion at a time. Between motions nothing is redrawn.
// When the turn of the session whose message box is open ends, it is happy
// for a moment.

type Frame = "idle" | "blink" | "tail-a" | "tail-b" | "ear";
type Motion = "blink" | "tail" | "ear";

/** One step of a motion: show `frame` for `ms` milliseconds. */
interface Step {
  readonly frame: Frame;
  readonly ms: number;
}

/** A motion repeats after a random delay between `min` and `max` ms. */
interface Schedule {
  readonly steps: readonly Step[];
  readonly min: number;
  readonly max: number;
}

/** The session whose message box is open, as `ui.composer` says. */
interface Composer {
  readonly machine: string;
  readonly session: string;
}

/** Where a session's turn stands. `done` is a session that waits for a prompt. */
type Turn = "running" | "done" | "other";

const WIDTH = 36;
const HEIGHT = 28;
const SCALE = 2;
const RETRY_MS = 400;
const HAPPY_MS = 1400;

const SCHEDULES: Record<Motion, Schedule> = {
  blink: {
    steps: [
      { frame: "blink", ms: 140 },
      { frame: "idle", ms: 0 },
    ],
    min: 3000,
    max: 7000,
  },
  tail: {
    steps: [
      { frame: "tail-a", ms: 200 },
      { frame: "tail-b", ms: 200 },
      { frame: "tail-a", ms: 200 },
      { frame: "tail-b", ms: 200 },
      { frame: "tail-a", ms: 200 },
      { frame: "idle", ms: 0 },
    ],
    min: 5000,
    max: 11000,
  },
  ear: {
    steps: [
      { frame: "ear", ms: 220 },
      { frame: "idle", ms: 120 },
      { frame: "ear", ms: 220 },
      { frame: "idle", ms: 0 },
    ],
    min: 6000,
    max: 13000,
  },
};

const MOTIONS: readonly Motion[] = ["blink", "tail", "ear"];

const frame = beep.state<Frame>("idle");
const happy = beep.state(false);
let playing = false;
let stopHappy: (() => void) | null = null;

function randomBetween(min: number, max: number): number {
  return min + Math.random() * (max - min);
}

/** Show each step in turn. Only one step timer is pending at a time. */
function play(steps: readonly Step[], index: number, done: () => void): void {
  const step = steps[index];
  if (step === undefined) {
    done();
    return;
  }

  frame.set(step.frame);
  if (index === steps.length - 1) {
    done();
    return;
  }
  beep.after(step.ms, () => play(steps, index + 1, done));
}

/** Wait a random time, then play the motion once nothing else is moving. */
function schedule(motion: Motion): void {
  const { steps, min, max } = SCHEDULES[motion];

  const attempt = (): void => {
    if (playing || happy.get()) {
      beep.after(RETRY_MS, attempt);
      return;
    }

    playing = true;
    play(steps, 0, () => {
      playing = false;
      schedule(motion);
    });
  };

  beep.after(randomBetween(min, max), attempt);
}

/** Show the happy frame for a moment. It hides the idle motions, which carry on after. */
function cheer(): void {
  stopHappy?.();
  happy.set(true);
  stopHappy = beep.after(HAPPY_MS, () => {
    stopHappy = null;
    happy.set(false);
  });
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
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

/**
 * Where the session's turn stands, from the `agent` topic. The app's view is
 * `{ Sessions: { Sessions: { sessions: [{ id, state: { status, subagents } }] } } }`.
 * A turn runs while the status is Working or Asking, or while subagents run.
 */
function readTurn(data: unknown, session: string): Turn | null {
  if (!isRecord(data) || !isRecord(data.Sessions)) {
    return null;
  }
  const reply = data.Sessions.Sessions;
  if (!isRecord(reply) || !Array.isArray(reply.sessions)) {
    return null;
  }

  const info: unknown = reply.sessions.find(
    (item: unknown) => isRecord(item) && item.id === session,
  );
  if (!isRecord(info) || !isRecord(info.state)) {
    return null;
  }

  const { status, subagents } = info.state;
  const waiting = typeof subagents === "number" && subagents > 0;
  if (status === "Working" || status === "Asking") {
    return "running";
  }
  if (status === "Idle") {
    return waiting ? "running" : "done";
  }

  return "other";
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
  beep.image({
    src: `art/cat-${happy.get() ? "happy" : frame.get()}.png`,
    width: WIDTH * SCALE,
    height: HEIGHT * SCALE,
    pixelated: true,
  }),
);

for (const motion of MOTIONS) {
  schedule(motion);
}
