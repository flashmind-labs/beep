/// <reference path="../../sdk/beep.d.ts" />

// A cat sits on the message box. Now and then it blinks, flicks its tail or
// twitches an ear, one motion at a time. Between motions nothing is redrawn.

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

const WIDTH = 36;
const HEIGHT = 28;
const SCALE = 2;
const RETRY_MS = 400;

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
let playing = false;

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
    if (playing) {
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

beep.place({ slot: "composer", align: "end", offset: 84, sink: 3 }, () =>
  beep.image({
    src: `art/cat-${frame.get()}.png`,
    width: WIDTH * SCALE,
    height: HEIGHT * SCALE,
    pixelated: true,
  }),
);

for (const motion of MOTIONS) {
  schedule(motion);
}
