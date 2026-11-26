/// When a motion plays: `loop` again and again (the only motion of its sprite), `once` when the
/// sprite appears, `every` after a random delay in the range and after each play.
type SpriteRepeat = { kind: "loop" } | { kind: "once" } | { kind: "every"; minMs: number; maxMs: number };

/// Show frame `frame` (an index into the sprite's frames) for `ms`.
interface SpriteStep { frame: number; ms: number }

/// Steps played in turn. After the last one the sprite goes back to its rest frame.
export interface SpriteMotion { steps: SpriteStep[]; repeat: SpriteRepeat }

/// Motions that play once, when the plugin asks for one by name.
export type SpriteReactions = Record<string, SpriteStep[]>;

/// A request to play the reaction `name` once. A new `id` asks again.
export interface SpritePlay { name: string; id: number }

const MAX_MOTIONS = 8;
const MAX_STEPS = 64;
export const MAX_FRAMES = 32;
const MIN_STEP_MS = 16;
const MAX_STEP_MS = 60_000;
const MIN_EVERY_MS = 100;
const MAX_EVERY_MS = 3_600_000;

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const isInt = (value: unknown, min: number, max: number): value is number => typeof value === "number" && Number.isInteger(value) && value >= min && value <= max;

function parseRepeat(raw: unknown): SpriteRepeat | null {
  if (!isRecord(raw)) return null;
  if (raw.kind === "loop") return { kind: "loop" };
  if (raw.kind === "once") return { kind: "once" };
  if (raw.kind !== "every") return null;
  const { minMs, maxMs } = raw;
  return isInt(minMs, MIN_EVERY_MS, MAX_EVERY_MS) && isInt(maxMs, MIN_EVERY_MS, MAX_EVERY_MS) && minMs <= maxMs ? { kind: "every", minMs, maxMs } : null;
}

function parseSteps(raw: unknown, frames: number): SpriteStep[] | null {
  if (!Array.isArray(raw) || raw.length === 0 || raw.length > MAX_STEPS) return null;
  const steps: SpriteStep[] = [];
  for (const step of raw) {
    if (!isRecord(step) || !isInt(step.frame, 0, frames - 1) || !isInt(step.ms, MIN_STEP_MS, MAX_STEP_MS)) return null;
    steps.push({ frame: step.frame, ms: step.ms });
  }
  return steps;
}

function parseMotion(raw: unknown, frames: number): SpriteMotion | null {
  if (!isRecord(raw)) return null;
  const steps = parseSteps(raw.steps, frames);
  const repeat = parseRepeat(raw.repeat);
  return steps === null || repeat === null ? null : { steps, repeat };
}

/// Reads a sprite's motions from the host, or null when any is invalid. `frames` is how many frames the sprite has.
export function parseMotions(raw: unknown, frames: number): SpriteMotion[] | null {
  if (!Array.isArray(raw) || raw.length > MAX_MOTIONS) return null;
  const motions: SpriteMotion[] = [];
  for (const item of raw) {
    const motion = parseMotion(item, frames);
    if (motion === null) return null;
    motions.push(motion);
  }
  // A loop plays all the time, so nothing else could.
  return motions.some(motion => motion.repeat.kind === "loop") && motions.length > 1 ? null : motions;
}

/// Reads a sprite's reactions, steps by name, or null when any is invalid.
export function parseReactions(raw: unknown, frames: number): SpriteReactions | null {
  if (!isRecord(raw) || Object.keys(raw).length > MAX_MOTIONS) return null;
  const reactions: [string, SpriteStep[]][] = [];
  for (const [name, reaction] of Object.entries(raw)) {
    const steps = parseSteps(reaction, frames);
    if (steps === null) return null;
    reactions.push([name, steps]);
  }
  return Object.fromEntries(reactions);
}

/// Whether `raw` asks for a reaction the sprite has.
export const isPlay = (raw: unknown, reactions: SpriteReactions): raw is SpritePlay =>
  isRecord(raw) && typeof raw.name === "string" && Object.hasOwn(reactions, raw.name) && isInt(raw.id, 0, 0xffffffff);

/// What to show now, and how long until `next` should be called. `wait` null: nothing more happens.
interface Tick { frame: number; wait: number | null }

/// The order a sprite's motions play in, as a state machine with no timers of its own.
export interface SpriteScript {
  /// Starts over at `now`, with every motion waiting for its time.
  begin(now: number): Tick;
  /// The wait of the last tick is over at `now`.
  next(now: number): Tick;
}

/// Motions play one at a time. A repeating motion's next play waits a random delay counted from the end of its last
/// play; a motion that is due while another plays waits its turn. `random` gives numbers in [0, 1).
export function createSpriteScript(motions: readonly SpriteMotion[], rest: number, random: () => number): SpriteScript {
  let playing: { motion: number; step: number } | null = null;
  let due: (number | null)[] = [];

  const delay = (repeat: SpriteRepeat) => repeat.kind === "every" ? repeat.minMs + random() * (repeat.maxMs - repeat.minMs) : 0;

  const show = (motion: number, step: number): Tick => {
    playing = { motion, step };
    const { frame, ms } = motions[motion].steps[step];
    return { frame, wait: ms };
  };

  const idle = (now: number): Tick => {
    let soonest = -1;
    let at: number | null = null;
    for (let index = 0; index < due.length; index++) {
      const when = due[index];
      if (when !== null && (at === null || when < at)) { soonest = index; at = when; }
    }
    if (at === null) return { frame: rest, wait: null };
    if (at > now) return { frame: rest, wait: at - now };

    due[soonest] = null;
    return show(soonest, 0);
  };

  return {
    begin(now) {
      playing = null;
      due = motions.map(motion => now + delay(motion.repeat));
      return idle(now);
    },
    next(now) {
      if (playing !== null) {
        const { motion, step } = playing;
        const { steps, repeat } = motions[motion];
        if (step + 1 < steps.length) return show(motion, step + 1);

        playing = null;
        if (repeat.kind === "loop") return show(motion, 0);
        if (repeat.kind === "every") due[motion] = now + delay(repeat);
      }
      return idle(now);
    },
  };
}

/// What a driver needs from its page. `setTimer` runs `fn` once after `ms`.
export interface SpriteHost {
  now(): number;
  setFrame(frame: number): void;
  setTimer(fn: () => void, ms: number): number;
  clearTimer(id: number): void;
}

/// Plays a script on screen: the frame changes only when a timer fires, one timer pending at a time,
/// and none while inactive. Inactive means hidden or reduced motion: the rest frame shows.
export interface SpriteDriver {
  setActive(active: boolean): void;
  /// Plays `steps` once now, then goes back to the script. Nothing happens while inactive.
  play(steps: readonly SpriteStep[]): void;
  /// Stops for good, without touching the frame.
  stop(): void;
}

export function createSpriteDriver(script: SpriteScript, rest: number, host: SpriteHost): SpriteDriver {
  let timer: number | null = null;
  let active = false;
  let reaction: { steps: readonly SpriteStep[]; at: number } | null = null;

  const clear = () => {
    if (timer !== null) host.clearTimer(timer);
    timer = null;
  };
  const run = (tick: Tick) => {
    host.setFrame(tick.frame);
    timer = tick.wait === null ? null : host.setTimer(fire, tick.wait);
  };
  const show = (steps: readonly SpriteStep[], at: number) => {
    reaction = { steps, at };
    run({ frame: steps[at].frame, wait: steps[at].ms });
  };
  const fire = () => {
    timer = null;
    if (!active) return;
    if (reaction === null) {
      run(script.next(host.now()));
      return;
    }

    const { steps, at } = reaction;
    if (at + 1 < steps.length) {
      show(steps, at + 1);
      return;
    }

    reaction = null;
    run(script.begin(host.now()));
  };

  return {
    setActive(next) {
      if (next === active) return;
      active = next;
      clear();
      reaction = null;
      if (next) run(script.begin(host.now()));
      else host.setFrame(rest);
    },
    play(steps) {
      if (!active || steps.length === 0) return;
      clear();
      show(steps, 0);
    },
    stop() {
      active = false;
      clear();
    },
  };
}
