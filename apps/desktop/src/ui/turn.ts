// A conversation's scrolling body, shared by the agent views and their search.

/// A chat's scrolling body, which knows whether it follows the newest turn.
export interface ChatBody extends HTMLElement {
  stuck?: boolean;
  /// When the reader last turned the wheel over it, by `performance.now()`.
  wheel?: number;
  /// A finger is still controlling the scroll.
  touching?: boolean;
  /// Where the reader last left it, kept while it is out of the page.
  top?: number;
  /// How far from its end it stood then, in pixels.
  gap?: number;
}
