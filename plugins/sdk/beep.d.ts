// Types for the global `beep` that the host gives every plugin.
// A plugin runs a JavaScript entry and relative .js imports inside its folder.
// Author TypeScript here, then build it to JavaScript before installing.
// It has no DOM and no setTimeout: use `beep.after` for timers. `console.log`,
// `console.warn` and `console.error` work like `beep.log`.

/**
 * A named place in the app. A slot can be on the page many times, each time
 * with its own context: `place` calls `render` once per instance.
 * - "composer": the edge of the session's message box.
 * - "session.row": a session's row in the session list. Text and images only.
 * - "session.header": the open session's header.
 */
type Slot = keyof SlotContexts;

/** The message box of a session. */
interface ComposerContext {
  readonly machine: string;
  readonly session: string;
}

/** A session's row in the session list. */
interface SessionRowContext {
  readonly machine: string;
  readonly session: string;
  /** The title the row shows. */
  readonly title: string;
}

/** The header of the open session. */
interface SessionHeaderContext {
  readonly machine: string;
  readonly session: string;
}

/** What `render` is given for each slot. */
interface SlotContexts {
  readonly composer: ComposerContext;
  readonly "session.row": SessionRowContext;
  readonly "session.header": SessionHeaderContext;
}

/** Which side of the slot a node is aligned to. */
type Align = "start" | "center" | "end";

/** How strongly text is drawn, in the app's colors. */
type Tone = "normal" | "dim" | "accent";

/** A picture. `src` is a path relative to the plugin folder. */
interface ImageNode {
  readonly kind: "image";
  readonly src: string;
  readonly width: number;
  readonly height: number;
  readonly pixelated: boolean;
}

/** One step of a motion: show `frame` (a path inside the plugin folder) for `ms`. */
interface SpriteStep {
  readonly frame: string;
  /** How long it shows, 16 to 60000 ms. */
  readonly ms: number;
}

/**
 * When a motion plays:
 * - "once" (default): once, when the sprite appears.
 * - "loop": again and again with no rest. It must be the sprite's only motion.
 * - `{ every: { min, max } }`: after a random delay between `min` and `max`
 *   ms (100 to 3600000), and again, with a new delay, after each play.
 */
type SpriteRepeat = "once" | "loop" | { readonly every: { readonly min: number; readonly max: number } };

/** Frames shown in turn. When the steps are done the sprite goes back to its rest frame. */
interface SpriteMotion {
  /** At most 64 steps. */
  readonly steps: readonly SpriteStep[];
  readonly repeat?: SpriteRepeat;
}

/** Frames that play once when the plugin asks for them by name; see `SpritePlay`. */
interface SpriteReaction {
  /** At most 64 steps. When they are done the sprite goes back to its motions. */
  readonly steps: readonly SpriteStep[];
}

/**
 * Asks a sprite to play its reaction `name`. The page plays it once for each new `id` and
 * ignores the same `id` on later draws, so keep the request in the node and change `id` to
 * ask again. A request that is already there when the sprite appears is not played.
 */
interface SpritePlay {
  readonly name: string;
  /** An integer, 0 to 4294967295. */
  readonly id: number;
}

/**
 * A picture the page animates by itself: it shows `rest`, and plays the
 * motions one at a time. The plugin is not woken for any frame. To change what
 * it shows, return another node after a state change. Draws only the rest
 * frame while the window is hidden or the user prefers reduced motion. A
 * reaction interrupts the motions, plays once, and the motions start over.
 */
interface SpriteNode {
  readonly kind: "sprite";
  readonly rest: string;
  readonly width: number;
  readonly height: number;
  readonly pixelated: boolean;
  readonly motions: readonly SpriteMotion[];
  readonly reactions?: Readonly<Record<string, SpriteReaction>>;
  readonly play?: SpritePlay;
}

/** A line of plain text. */
interface TextNode {
  readonly kind: "text";
  readonly text: string;
  readonly tone?: Tone;
}

/** Children laid out left to right with `gap` px between them. */
interface RowNode {
  readonly kind: "row";
  readonly gap: number;
  readonly children: readonly Node[];
}

/**
 * A button. `label` is its accessible name, and its content when it has no
 * children. Not allowed in "session.row".
 */
interface ButtonNode {
  readonly kind: "button";
  readonly label: string;
  readonly children: readonly Node[];
  readonly onPress: () => void;
}

/** A switch. Not allowed in "session.row". */
interface ToggleNode {
  readonly kind: "toggle";
  readonly label: string;
  readonly value: boolean;
  readonly onChange: (value: boolean) => void;
}

/** A single line of text. Not allowed in "session.row". */
interface InputNode {
  readonly kind: "input";
  readonly label: string;
  readonly value: string;
  readonly placeholder: string;
  readonly onChange?: (value: string) => void;
  readonly onSubmit?: (value: string) => void;
}

/** Anything a plugin can draw. */
type Node = ImageNode | SpriteNode | TextNode | RowNode | ButtonNode | ToggleNode | InputNode;

/** Options for `beep.image`. Sizes are CSS px. */
interface ImageOptions {
  /** Path inside the plugin folder, such as "art/cat-idle.png". Max 256 KB. */
  src: string;
  width: number;
  height: number;
  /** Scale without smoothing. Use it for pixel art. Default false. */
  pixelated?: boolean;
}

/** Options for `beep.sprite`. Sizes are CSS px. */
interface SpriteOptions {
  /** Path of the frame shown when no motion plays. Max 256 KB. */
  rest: string;
  width: number;
  height: number;
  /** Scale without smoothing. Use it for pixel art. Default false. */
  pixelated?: boolean;
  /** At most 8 motions and 32 distinct frames in all. Default none. */
  motions?: readonly SpriteMotion[];
  /** Motions played on request, by name. At most 8; their frames count toward the 32. Default none. */
  reactions?: Readonly<Record<string, SpriteReaction>>;
  /** The reaction to play now. It must name one of `reactions`. Default none. */
  play?: SpritePlay;
}

/** Options for `beep.row`. */
interface RowOptions {
  /** Space between children in px. Default 0. */
  gap?: number;
}

/** Options for `beep.text`. */
interface TextOptions {
  /** Default "normal". */
  tone?: Tone;
}

/** Options for `beep.button`. */
interface ButtonOptions {
  /** The accessible name. It is the content too when there are no children. */
  label: string;
  /** Runs when the button is pressed. */
  onPress: () => void;
}

/** Options for `beep.toggle`. */
interface ToggleOptions {
  label: string;
  value: boolean;
  /** Runs with the new value when the user switches it. Set the state there. */
  onChange: (value: boolean) => void;
}

/** Options for `beep.input`. */
interface InputOptions {
  label: string;
  /** The text shown. Keep it in a state and set it in `onChange`. Default "". */
  value?: string;
  placeholder?: string;
  /** Runs with the whole text on each change. At most 4 KB. */
  onChange?: (value: string) => void;
  /** Runs with the whole text when the user presses Enter. */
  onSubmit?: (value: string) => void;
}

/** How a placement comes and goes: a short (about 150 ms) fade or rise, or nothing. Skipped under reduced motion. */
type Transition = "none" | "fade" | "rise";

/** Options every slot takes. */
interface PlaceTransitions {
  /** How the placement appears. Default "none". */
  enter?: Transition;
  /** How it goes when the plugin stops drawing it. Default "none". */
  leave?: Transition;
}

/** The composer stands on the edge of the message box. */
interface ComposerPlace extends PlaceTransitions {
  slot: "composer";
  /** Side of the slot to align to. Default "end". */
  align?: Align;
  /** Distance in px from the aligned side. Default 0. */
  offset?: number;
  /** Distance in px the node sinks below the slot's edge. Default 0. */
  sink?: number;
}

/** Other slots draw in the page's normal flow and take no position. */
interface InlinePlace<S extends Exclude<Slot, "composer">> extends PlaceTransitions {
  slot: S;
}

/**
 * A topic to follow, as the app names it. Topics:
 * - "ui.composer": the session whose message box is shown. `data` is
 *   `{ machine: string, session: string }`, or null when no message box is shown.
 *   The "composer" slot's context says the same. Other "ui." names are the
 *   app's own state. No permission needed.
 * - "session.turn": where a session's turn stands. Name the session with `key` and its
 *   `machine`. Needs "sessions.read". `data` is a `SessionTurn`. It covers the sessions
 *   the app follows (the open one, and ones that run or ask); the latest value arrives
 *   right away if the app has one.
 * - "agents": the sessions of every machine, or of `machine`. Needs "sessions.read".
 * - "agent": one session, named by `key` (a session id), or every session without
 *   a key. Needs "sessions.read".
 * Any other name is refused. `session.turn` and "ui." topics are stable. The shape of
 * "agents" and "agent" `data` is the app's internal view and can change between releases.
 */
interface Topic {
  readonly name: string;
  /** The resource the topic is about, such as a session id. Default: all. */
  readonly key?: string | null;
  /** Only this machine's updates. Default: every machine. */
  readonly machine?: string | null;
}

/** Why a turn ended, as "session.turn" reports it. */
type TurnOutcome =
  | "finished"
  | "cancelled"
  | "failed"
  | "refused"
  | "token-limit"
  | "request-limit"
  | "stopped";

/** The `data` of "session.turn". */
interface SessionTurn {
  readonly session: string;
  /** "working" covers a session that waits on subagents. */
  readonly state: "starting" | "working" | "asking" | "idle" | "ended";
  /** How the last turn ended, null before the first. */
  readonly outcome: TurnOutcome | null;
  /**
   * How many turns have ended. A larger number than the last update is a new end;
   * the same number is a repeat. The first update may describe an end that came
   * before you followed.
   */
  readonly turn: number;
}

/** One update to a topic. */
interface TopicEvent {
  /** The topic that was followed. */
  readonly topic: {
    readonly name: string;
    readonly key: string | null;
    readonly machine: string | null;
  };
  /** The machine the update came from, null for "ui." topics. */
  readonly machine: string | null;
  /** The new value. Narrow it before use: it is not typed. */
  readonly data: unknown;
}

/** A value the UI depends on. `set` marks the UI dirty so it is drawn again. */
interface State<T> {
  get(): T;
  set(next: T): void;
}

/**
 * Values kept for the plugin across restarts, as JSON, in a file of the plugins
 * folder. Changes are written a moment later and when the plugin stops. All keys
 * and values together may take 256 KB. No permission needed.
 */
interface Store {
  /** The value saved under `key`, or null when there is none. */
  get(key: string): unknown;
  /** Save a JSON value. Throws when it is not JSON or the store would pass 256 KB. */
  set(key: string, value: unknown): void;
  delete(key: string): void;
}

interface Beep {
  /** Optional APIs included in this build. Both this object and `beep` are frozen. */
  readonly capabilities: { readonly tools: boolean };
  readonly store: Store;

  /**
   * Write a line to the app's log file, at info. Values are formatted like
   * `console.log`: strings as they are, the rest as JSON. A line is cut at 1 KB,
   * and a plugin may write 20 lines a second.
   */
  log(...values: unknown[]): void;

  /**
   * Put a node in a slot. `render` runs once for each instance of the slot on
   * the page, with that instance's context. It runs again for an instance whose
   * context changed, and for all of them after a state was set. Return null to
   * draw nothing for an instance. Up to 200 instances of a slot are drawn, and
   * 8 `place` calls are allowed. Needs the "ui.add" permission.
   */
  place(options: ComposerPlace, render: (context: ComposerContext) => Node | null): void;
  place(options: InlinePlace<"session.row">, render: (context: SessionRowContext) => Node | null): void;
  place(options: InlinePlace<"session.header">, render: (context: SessionHeaderContext) => Node | null): void;

  /** Create a state with an initial value. */
  state<T>(initial: T): State<T>;

  /**
   * Run `callback` once after `ms` milliseconds (at least 16). Timers wait
   * while no window is visible, then overdue ones run once each.
   * Returns a function that cancels it. At most 64 timers can be pending.
   */
  after(ms: number, callback: () => void): () => void;

  /**
   * Call `callback` with each update to `topic`, then draw again if state
   * changed. For a "ui." topic the latest value arrives right away if one was
   * published. When updates pile up, only the newest per topic and machine is
   * delivered. Returns a function that stops it. At most 16 subscriptions.
   * Throws for a topic that needs a permission the plugin lacks, and for
   * unknown names.
   */
  on(topic: Topic, callback: (event: TopicEvent) => void): () => void;

  /**
   * Give coding agents a tool. `name` must be declared in the manifest's `tools`
   * with a description and an `input` JSON Schema, and every declared tool must be
   * registered, or the plugin does not load. The handler gets the arguments, already
   * checked against the schema, and returns a JSON value or a promise of one. It runs
   * on the plugin's thread under the usual CPU budget; a promise that does not settle
   * within 10 seconds fails the call, not the plugin, and a thrown error is the
   * call's error. Results over 64 KB fail. Needs the "tools" permission.
   * Check `beep.capabilities.tools` first; unavailable builds refuse registration.
   */
  tool<Input = unknown>(name: string, handler: (input: Input) => unknown): void;

  /** Build an image node. */
  image(options: ImageOptions): Node;

  /**
   * Build a sprite node: a picture the page animates itself, with no timers or
   * redraws in the plugin. A plugin's images may total 4 MB.
   */
  sprite(options: SpriteOptions): Node;

  /** Build a text node. */
  text(text: string, options?: TextOptions): Node;

  /** Build a row node. */
  row(options: RowOptions, children: readonly Node[]): Node;

  /**
   * Build a button. Its callback runs on the plugin's thread, then the UI is
   * drawn again. A callback from an older draw is ignored. A plugin gets at
   * most 30 events a second.
   */
  button(options: ButtonOptions, children?: readonly Node[]): Node;

  /** Build a switch. */
  toggle(options: ToggleOptions): Node;

  /** Build a single line text field. */
  input(options: InputOptions): Node;
}

/** Like `beep.log`, at the level of the method: info, warn or error. */
declare const console: {
  log(...values: unknown[]): void;
  warn(...values: unknown[]): void;
  error(...values: unknown[]): void;
};

declare const beep: Beep;
