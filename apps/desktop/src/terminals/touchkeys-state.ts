// The touch key bar's key logic, ported from the phone apps'
// TerminalKeys.swift / TerminalKeys.kt. Output is the string the terminal
// hands to onData.

export type TermMod = "ctrl" | "alt" | "shift";
export type TermKey =
  | "esc" | "tab" | "up" | "down" | "left" | "right" | "home" | "end"
  | { char: string };

/// The bar's two rows of nine; "" is an empty cell, which keeps the arrows an inverted T.
export const KEY_ROWS: readonly (readonly string[])[] = [
  ["Esc", "Tab", "|", "/", "~", "-", "", "↑", ""],
  ["Ctrl", "Alt", "Shift", "Home", "End", "", "←", "↓", "→"],
];

const MOD_LABELS: Record<string, TermMod> = { Ctrl: "ctrl", Alt: "alt", Shift: "shift" };
const NAMED: Record<string, TermKey> = {
  Esc: "esc", Tab: "tab", Home: "home", End: "end", "↑": "up", "↓": "down", "←": "left", "→": "right",
};

/// What a bar label is: a modifier, a named key, or a literal character.
export function labelKey(label: string): { mod: TermMod } | { key: TermKey } {
  const mod = MOD_LABELS[label];
  if (mod) return { mod };
  return { key: NAMED[label] ?? { char: label } };
}

/// The bytes a key sends. Arrows, Home and End carry the xterm modifier number (Shift 2, Alt 3, Ctrl 5).
export function keyBytes(key: TermKey, mods: ReadonlySet<TermMod>): string {
  let number = 1;
  if (mods.has("shift")) number += 1;
  if (mods.has("alt")) number += 2;
  if (mods.has("ctrl")) number += 4;
  const csi = (final: string) => (number === 1 ? `\x1b[${final}` : `\x1b[1;${number}${final}`);

  switch (key) {
    case "esc": return "\x1b";
    case "tab": return mods.has("shift") ? "\x1b[Z" : "\t";
    case "up": return csi("A");
    case "down": return csi("B");
    case "right": return csi("C");
    case "left": return csi("D");
    case "home": return csi("H");
    case "end": return csi("F");
  }

  let out = mods.has("shift") ? key.char.toUpperCase() : key.char;

  // Ctrl with a letter or one of @ [ \ ] ^ _ is the control byte.
  if (mods.has("ctrl") && out.length === 1) {
    const code = out.charCodeAt(0);
    if (code >= 0x40 && code <= 0x7f) out = String.fromCharCode(code & 0x1f);
  }

  return mods.has("alt") ? `\x1b${out}` : out;
}

/// Ctrl, Alt and Shift stay armed for one key.
export class TermKeyState {
  readonly mods = new Set<TermMod>();

  toggle(mod: TermMod): void {
    if (!this.mods.delete(mod)) this.mods.add(mod);
  }

  /// Soft keyboard input. Only a lone printable ASCII character takes the armed
  /// modifiers; anything longer (pastes, replies from the emulator) passes
  /// unchanged and leaves them armed.
  applyTyped(typed: string): string {
    if (this.mods.size === 0 || typed.length !== 1) return typed;
    const code = typed.charCodeAt(0);
    if (code < 0x20 || code >= 0x7f) return typed;
    return this.press({ char: typed });
  }

  press(key: TermKey): string {
    const out = keyBytes(key, this.mods);
    this.mods.clear();
    return out;
  }
}
