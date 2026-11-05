// On macOS a Ctrl+click opens the context menu and the web view may still send a
// `click` for it, which a row would take as a plain click and open its session.

/// What a click is, as far as the guard needs.
export interface ClickInfo {
  button: number;
  ctrlKey: boolean;
  /// The target is a text field, the terminal or a link, which keep their own Ctrl+click.
  native: boolean;
}

/// Whether a click is dropped: not the primary button, or a Ctrl+click, except on
/// something that handles it itself.
export function swallowsClick(c: ClickInfo) {
  if (c.native) return false;
  return c.button !== 0 || c.ctrlKey;
}
