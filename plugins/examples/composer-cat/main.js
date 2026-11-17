// Generated from main.ts with npm run plugins:build.

// main.ts
var WIDTH = 36;
var HEIGHT = 28;
var SCALE = 2;
var art = (name) => `art/cat-${name}.png`;
var IDLE = [
  {
    steps: [{ frame: art("blink"), ms: 140 }],
    repeat: { every: { min: 3e3, max: 7e3 } }
  },
  {
    steps: [
      { frame: art("tail-a"), ms: 200 },
      { frame: art("tail-b"), ms: 200 },
      { frame: art("tail-a"), ms: 200 },
      { frame: art("tail-b"), ms: 200 },
      { frame: art("tail-a"), ms: 200 }
    ],
    repeat: { every: { min: 5e3, max: 11e3 } }
  },
  {
    steps: [
      { frame: art("ear"), ms: 220 },
      { frame: art("idle"), ms: 120 },
      { frame: art("ear"), ms: 220 }
    ],
    repeat: { every: { min: 6e3, max: 13e3 } }
  }
];
var TYPING = [
  {
    steps: [
      { frame: art("type-a"), ms: 140 },
      { frame: art("type-b"), ms: 140 }
    ],
    repeat: "loop"
  }
];
var REACTIONS = {
  happy: { steps: [{ frame: art("happy"), ms: 1400 }] },
  startled: { steps: [{ frame: art("startled"), ms: 900 }] },
  sad: { steps: [{ frame: art("sad"), ms: 1400 }] },
  yawn: {
    steps: [
      { frame: art("yawn-a"), ms: 500 },
      { frame: art("yawn-b"), ms: 900 },
      { frame: art("yawn-a"), ms: 500 }
    ]
  }
};
var REACTION = {
  finished: "happy",
  cancelled: "startled",
  failed: "sad",
  refused: "sad",
  "token-limit": "yawn",
  "request-limit": "yawn",
  stopped: "yawn"
};
var isState = (value) => value === "starting" || value === "working" || value === "asking" || value === "idle" || value === "ended";
var isOutcome = (value) => typeof value === "string" && Object.keys(REACTION).includes(value);
var turnState = beep.state("idle");
var play = beep.state(null);
var plays = 0;
function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function readComposer(data) {
  if (!isRecord(data)) {
    return null;
  }
  const { machine, session } = data;
  if (typeof machine !== "string" || typeof session !== "string") {
    return null;
  }
  return { machine, session };
}
function readTurn(data) {
  if (!isRecord(data)) {
    return null;
  }
  const { session, state, outcome, turn } = data;
  if (typeof session !== "string" || !isState(state) || typeof turn !== "number" || !(outcome === null || isOutcome(outcome))) {
    return null;
  }
  return { session, state, outcome, turn };
}
var followed = null;
var unfollow = null;
function follow(composer) {
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
  let seen = null;
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
    }
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
    ...playing === null ? {} : { play: playing }
  });
});
