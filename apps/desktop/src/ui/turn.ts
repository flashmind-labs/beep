// The shapes of a chat turn that the bar, the window and the agent views draw.

/// A tool step of a turn, as the chat shows it. A step still running when
/// its turn ends is marked stopped.
export interface Step {
  id: string;
  /// The tool's name, for its row's icon.
  name: string;
  label: string;
  state: "run" | "ok" | "bad" | "stop";
}

/// The model's thought in a turn: its text so far, when it started and, once
/// something else came, how long it took. `touched` and `open` keep the
/// reader's choice of folding it; `ended` that it folded once already.
export interface TurnThought {
  text: string;
  start: number;
  ms?: number;
  touched?: boolean;
  open?: boolean;
  ended?: boolean;
}

/// A mounted chat turn, used as a handle to its view.
export interface Turn extends HTMLDivElement {}

/// A chat's scrolling body, which knows whether it follows the newest turn.
export interface ChatBody extends HTMLElement {
  stuck?: boolean;
  /// When the reader last turned the wheel over it, by `performance.now()`.
  wheel?: number;
  /// Where the reader last left it, kept while it is out of the page.
  top?: number;
}
