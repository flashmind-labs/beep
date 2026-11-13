// Types for the global `beep` that the host gives every plugin.
// A plugin is one source file with no imports. It has no DOM, no
// setTimeout and no console: use `beep.after` for timers.

/** Where a plugin can place a node. Only the message box exists for now. */
type Slot = "composer";

/** Which side of the slot a node is aligned to. */
type Align = "start" | "center" | "end";

/** A picture. `src` is a path relative to the plugin folder. */
interface ImageNode {
  readonly kind: "image";
  readonly src: string;
  readonly width: number;
  readonly height: number;
  readonly pixelated: boolean;
}

/** A line of plain text. */
interface TextNode {
  readonly kind: "text";
  readonly text: string;
}

/** Children laid out left to right with `gap` px between them. */
interface RowNode {
  readonly kind: "row";
  readonly gap: number;
  readonly children: readonly Node[];
}

/** Anything a plugin can draw. */
type Node = ImageNode | TextNode | RowNode;

/** Options for `beep.image`. Sizes are CSS px. */
interface ImageOptions {
  /** Path inside the plugin folder, such as "art/cat-idle.png". Max 256 KB. */
  src: string;
  width: number;
  height: number;
  /** Scale without smoothing. Use it for pixel art. Default false. */
  pixelated?: boolean;
}

/** Options for `beep.row`. */
interface RowOptions {
  /** Space between children in px. Default 0. */
  gap?: number;
}

/** Where `beep.place` puts the node. */
interface PlaceOptions {
  slot: Slot;
  /** Side of the slot to align to. Default "end". */
  align?: Align;
  /** Distance in px from the aligned side. Default 0. */
  offset?: number;
  /** Distance in px the node sinks below the slot's edge. Default 0. */
  sink?: number;
}

/** A value the UI depends on. `set` marks the UI dirty so it is drawn again. */
interface State<T> {
  get(): T;
  set(next: T): void;
}

interface Beep {
  /**
   * Put a node in a slot. `render` runs again whenever a state changed.
   * Needs the "ui.add" permission.
   */
  place(options: PlaceOptions, render: () => Node): void;

  /** Create a state with an initial value. */
  state<T>(initial: T): State<T>;

  /**
   * Run `callback` once after `ms` milliseconds (at least 16).
   * Returns a function that cancels it. At most 64 timers can be pending.
   */
  after(ms: number, callback: () => void): () => void;

  /** Build an image node. */
  image(options: ImageOptions): Node;

  /** Build a text node. */
  text(text: string): Node;

  /** Build a row node. */
  row(options: RowOptions, children: readonly Node[]): Node;
}

declare const beep: Beep;
