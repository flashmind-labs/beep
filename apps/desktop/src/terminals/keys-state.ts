// Word and line keys for a terminal screen, as the bytes a shell reads.
//
// Keys handled (Cmd is the meta key):
//   Ctrl+Left, Option+Left        word back          ESC b
//   Ctrl+Right, Option+Right      word forward       ESC f
//   Cmd+Left, Cmd+Right           line start, end    ^A, ^E
//   Option+Backspace              delete word back   ESC DEL
//   Cmd+Backspace                 delete to start    ^U
//   Option+Delete (fn+Option+Bksp) delete word next  ESC d
// ESC b/f/d/DEL work in readline, zsh and fish. PowerShell's PSReadLine
// (default Windows mode) binds Ctrl+Left/Right itself but not ESC b/f, and
// takes ^A as select-all, so on Windows nodes these keys may do less.

type KeyLike = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">;

/// The bytes to send for a key, or null when xterm.js handles it.
export function wordKeyBytes(e: KeyLike): string | null {
  if (e.shiftKey) return null;
  const { metaKey: meta, ctrlKey: ctrl, altKey: alt } = e;
  switch (e.key) {
    case "ArrowLeft":
      if (meta && !ctrl && !alt) return "\x01";
      if (ctrl !== alt && !meta) return "\x1bb";
      return null;
    case "ArrowRight":
      if (meta && !ctrl && !alt) return "\x05";
      if (ctrl !== alt && !meta) return "\x1bf";
      return null;
    case "Backspace":
      if (meta && !ctrl && !alt) return "\x15";
      if (alt && !meta && !ctrl) return "\x1b\x7f";
      return null;
    case "Delete":
      if (alt && !meta && !ctrl) return "\x1bd";
      return null;
    default:
      return null;
  }
}
