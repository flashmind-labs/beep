// The skin every window draws in: a set of color, corner, font and label
// tokens (dist/skins.css). The pick is kept in localStorage, which all of
// the app's windows share, so a change in one shows in the others at once.

export interface Skin {
  id: string;
  name: string;
  /// A color that stands for it in the picker.
  swatch: string;
  light?: boolean;
}

export const SKINS: readonly Skin[] = [
  { id: "linear", name: "Linear", swatch: "#151617" },
  { id: "classic", name: "Classic", swatch: "#1a2a23" },
  { id: "quiet", name: "Quiet", swatch: "#161b18" },
  { id: "github", name: "GitHub dark", swatch: "#161b22" },
  { id: "zed", name: "Zed", swatch: "#2f343e" },
  { id: "tokyo", name: "Tokyo Night", swatch: "#1f2335" },
  { id: "mocha", name: "Catppuccin Mocha", swatch: "#313244" },
  { id: "nord", name: "Nord", swatch: "#3b4252" },
  { id: "gruvbox", name: "Gruvbox", swatch: "#3c3836" },
  { id: "rosepine", name: "Rose Pine", swatch: "#26233a" },
  { id: "geist", name: "Geist", swatch: "#000" },
  { id: "mono", name: "High contrast", swatch: "#000" },
  { id: "paper", name: "Light", swatch: "#f3f3f1", light: true },
  { id: "solarized", name: "Solarized", swatch: "#eee8d5", light: true },
  { id: "macos", name: "macOS", swatch: "#e9e7ea", light: true },
  { id: "warm", name: "Warm", swatch: "#f3efe8", light: true },
];

export const DEFAULT_SKIN = "linear";
const KEY = "beep.skin";

/// The picked skin's id, the default when none or an unknown one is kept.
export function skinId(): string {
  let kept: string | null = null;
  try { kept = localStorage.getItem(KEY); } catch {}
  return SKINS.some(s => s.id === kept) ? kept! : DEFAULT_SKIN;
}

function apply() {
  const id = skinId();
  if (id === DEFAULT_SKIN) delete document.documentElement.dataset.skin;
  else document.documentElement.dataset.skin = id;
}

/// Picks skin `id` for every window.
export function setSkin(id: string) {
  try { localStorage.setItem(KEY, id); } catch {}
  apply();
}

/// Draws this window in the picked skin, and follows picks made in others.
export function followSkin() {
  apply();
  addEventListener("storage", e => { if (e.key === KEY) apply(); });
}
