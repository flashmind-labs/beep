// The skin every window draws in: a set of style tokens (dist/skins.css). The
// pick is kept in localStorage, which all of the app's windows share, so a
// change in one shows in the others at once. Plugins with the style permission
// set tokens over the skin. Also the layout picked for Home and a project's
// front page.
import { parsePluginStyle } from "../settings/plugins-settings-state.ts";
import { APPLE, onPhone } from "./platform.ts";
import { invoke, listen } from "./tauri.ts";

interface Skin {
  id: string;
  name: string;
  /// A color that stands for it in the picker.
  swatch: string;
  light?: boolean;
}

export const SKINS: readonly Skin[] = [
  { id: "flashmind", name: "Flashmind", swatch: "#0d161e" },
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

const DEFAULT_SKIN = "linear";
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

  // Windows and Linux draw the title bar: it takes the skin's light or dark.
  if (APPLE || onPhone()) return;
  const light = SKINS.find(s => s.id === id)?.light ?? false;
  window.__TAURI__.webviewWindow.getCurrentWebviewWindow().setTheme?.(light ? "light" : "dark").catch(() => {});
}

/// Picks skin `id` for every window.
export function setSkin(id: string) {
  try { localStorage.setItem(KEY, id); } catch {}
  apply();
}

/// Draws this window in the picked skin with the plugins' tokens over it, and follows
/// picks made in others and plugins turned on or off.
export function followSkin() {
  apply();
  addEventListener("storage", e => { if (e.key === KEY) apply(); });

  void listen("plugins-status", () => { void applyPluginStyle(); }).catch(() => {});
  void applyPluginStyle();
}

/// The tokens plugins set on this window, to clear the ones they stop setting.
let pluginTokens: readonly string[] = [];
let styleReads = 0;

async function applyPluginStyle() {
  const mine = ++styleReads;
  const style = parsePluginStyle(await invoke<unknown>("plugins_style").catch(() => null));
  if (style === null || mine !== styleReads) return;

  const root = document.documentElement.style;
  for (const token of pluginTokens) if (!style.tokens.has(token)) root.removeProperty(token);
  for (const [token, value] of style.tokens) root.setProperty(token, value);
  pluginTokens = [...style.tokens.keys()];
}

/// How Home and a project's front page lay out, each on its own: the board, or the
/// sections and rows before it. Picked in Settings, under Appearance.
export type FrontLayout = "board" | "list";
export type FrontPage = "home" | "project";
const layoutKey = (page: FrontPage) => `beep.layout.${page}`;
const kept = (page: FrontPage): FrontLayout => { try { return localStorage.getItem(layoutKey(page)) === "list" ? "list" : "board"; } catch { return "board"; } };
const layouts: Record<FrontPage, FrontLayout> = { home: kept("home"), project: kept("project") };
export const frontLayout = (page: FrontPage) => layouts[page];
/// Called when the user picks a layout: every page that shows it draws again.
export const layoutChanged = new Set<() => void>();

export function setFrontLayout(page: FrontPage, next: FrontLayout) {
  if (next === layouts[page]) return;
  layouts[page] = next;
  try { localStorage.setItem(layoutKey(page), next); } catch { /* kept for this launch only */ }
  for (const draw of layoutChanged) draw();
}
