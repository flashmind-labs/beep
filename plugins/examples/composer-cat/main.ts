/// <reference path="../../sdk/beep.d.ts" />

// A cat sits on the message box. Now and then it blinks, flicks its tail or
// twitches an ear, one motion at a time. While the session works it types, and
// while it asks a question the cat's ears are up. When a turn ends the cat
// reacts once, by how it ended. The page plays all of this from declarations,
// so the plugin sleeps between events.

/** The session whose message box is open, as `ui.composer` says. */
interface Composer {
  readonly machine: string;
  readonly session: string;
}

const WIDTH = 36;
const HEIGHT = 28;
const SCALE = 2;

const art = (name: string): string => `art/cat-${name}.png`;

/**
 * The idle motions are declared, and the page plays them: each starts after a
 * random delay in its range, one at a time, and the cat rests on its idle
 * frame in between. Nothing here wakes the plugin.
 */
const IDLE: readonly SpriteMotion[] = [
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

/** Paws on the keys for as long as the turn runs. */
const TYPING: readonly SpriteMotion[] = [
  {
    steps: [
      { frame: art("type-a"), ms: 140 },
      { frame: art("type-b"), ms: 140 },
    ],
    repeat: "loop",
  },
];

const REACTIONS: Readonly<Record<string, SpriteReaction>> = {
  happy: { steps: [{ frame: art("happy"), ms: 1400 }] },
  startled: { steps: [{ frame: art("startled"), ms: 900 }] },
  sad: { steps: [{ frame: art("sad"), ms: 1400 }] },
  yawn: {
    steps: [
      { frame: art("yawn-a"), ms: 500 },
      { frame: art("yawn-b"), ms: 900 },
      { frame: art("yawn-a"), ms: 500 },
    ],
  },
};

/** The reaction to each way a turn can end. */
const REACTION: Readonly<Record<TurnOutcome, string>> = {
  finished: "happy",
  cancelled: "startled",
  failed: "sad",
  refused: "sad",
  "token-limit": "yawn",
  "request-limit": "yawn",
  stopped: "yawn",
};

const isState = (value: unknown): value is SessionTurn["state"] =>
  value === "starting" ||
  value === "working" ||
  value === "asking" ||
  value === "idle" ||
  value === "ended";

const isOutcome = (value: unknown): value is TurnOutcome =>
  typeof value === "string" && Object.keys(REACTION).includes(value);

const turnState = beep.state<SessionTurn["state"]>("idle");
const play = beep.state<SpritePlay | null>(null);
let plays = 0;

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

/** A `SessionTurn` from the `session.turn` topic, or null when it is anything else. */
function readTurn(data: unknown): SessionTurn | null {
  if (!isRecord(data)) {
    return null;
  }
  const { session, state, outcome, turn } = data;
  if (
    typeof session !== "string" ||
    !isState(state) ||
    typeof turn !== "number" ||
    !(outcome === null || isOutcome(outcome))
  ) {
    return null;
  }

  return { session, state, outcome, turn };
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
  turnState.set("idle");
  play.set(null);
  if (composer === null) {
    return;
  }

  // The first update only says where the turn stands: an end that came before is old news.
  let seen: number | null = null;
  unfollow = beep.on(
    { name: "session.turn", key: composer.session, machine: composer.machine },
    (event) => {
      const turn = readTurn(event.data);
      if (turn === null) {
        return;
      }

      turnState.set(turn.state);
      if (seen !== null && turn.turn > seen && turn.outcome !== null) {
        play.set({ name: REACTION[turn.outcome], id: ++plays });
      }
      seen = turn.turn;
    },
  );
}

beep.on({ name: "ui.composer" }, (event) => follow(readComposer(event.data)));

beep.place({ slot: "composer", align: "end", offset: 84, sink: 3, enter: "rise" }, () => {
  const now = turnState.get();
  const playing = play.get();

  return beep.sprite({
    rest: art(now === "asking" ? "ask" : "idle"),
    width: WIDTH * SCALE,
    height: HEIGHT * SCALE,
    pixelated: true,
    motions: now === "working" ? TYPING : now === "asking" ? [] : IDLE,
    reactions: REACTIONS,
    ...(playing === null ? {} : { play: playing }),
  });
});
