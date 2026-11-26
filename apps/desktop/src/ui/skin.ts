// The skin every window draws in: a set of style tokens (dist/skins.css), or
// the tokens of a skin plugin over the default. The pick is kept in
// localStorage, which all of the app's windows share, so a change in one shows
// in the others at once. Also the layout picked for Home and a project's front
// page.
import { createStore } from "zustand";
import { parseRows, pluginSkins, type PluginSkin } from "./plugin-rows.ts";
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
/// The picked plugin skin's tokens, so a window shows them before the plugins are read.
const TOKENS_KEY = "beep.skin.tokens";
/// A plugin skin's id in the pick.
const PLUGIN = "plugin:";

/// The skins installed plugins offer, read from the desktop. Empty on a phone.
export const skinPlugins = createStore<{ skins: readonly PluginSkin[] }>(() => ({ skins: [] }));
let pluginsRead = false;

export const pluginSkinId = (plugin: string) => `${PLUGIN}${plugin}`;

/// The picked skin's id, the default when none or an unknown one is kept.
export function skinId(): string {
  let kept: string | null = null;
  try { kept = localStorage.getItem(KEY); } catch {}
  if (kept === null) return DEFAULT_SKIN;
  if (kept.startsWith(PLUGIN)) return !pluginsRead || skinPlugins.getState().skins.some(s => pluginSkinId(s.id) === kept) ? kept : DEFAULT_SKIN;
  return SKINS.some(s => s.id === kept) ? kept : DEFAULT_SKIN;
}

/// Called when this window takes another skin: what draws in its colours
/// outside CSS, such as a terminal, draws again.
export const skinChanged = new Set<() => void>();

/// The tokens this window set from a plugin skin, to clear them on the next pick.
let pluginTokens: readonly string[] = [];

function apply() {
  const id = skinId();
  const builtin = id.startsWith(PLUGIN) ? DEFAULT_SKIN : id;
  if (builtin === DEFAULT_SKIN) delete document.documentElement.dataset.skin;
  else document.documentElement.dataset.skin = builtin;

  const tokens = id.startsWith(PLUGIN) ? pluginTokensOf(id) : new Map<string, string>();
  const root = document.documentElement.style;
  for (const token of pluginTokens) if (!tokens.has(token)) root.removeProperty(token);
  for (const [token, value] of tokens) root.setProperty(token, value);
  pluginTokens = [...tokens.keys()];
  for (const draw of skinChanged) draw();

  // Windows and Linux draw the title bar: it takes the skin's light or dark.
  if (APPLE || onPhone()) return;
  const light = SKINS.find(s => s.id === builtin)?.light ?? false;
  window.__TAURI__.webviewWindow.getCurrentWebviewWindow().setTheme?.(light ? "light" : "dark").catch(() => {});
}

/// A plugin skin's tokens: from the plugins once read, and kept for the next launch.
function pluginTokensOf(id: string): Map<string, string> {
  const skin = skinPlugins.getState().skins.find(s => pluginSkinId(s.id) === id);
  if (skin !== undefined) {
    try { localStorage.setItem(TOKENS_KEY, JSON.stringify([...skin.tokens])); } catch {}
    return new Map(skin.tokens);
  }

  try {
    const kept: unknown = JSON.parse(localStorage.getItem(TOKENS_KEY) ?? "[]");
    if (Array.isArray(kept)) return new Map(kept.filter((pair): pair is [string, string] => Array.isArray(pair) && typeof pair[0] === "string" && pair[0].startsWith("--") && typeof pair[1] === "string"));
  } catch {}
  return new Map();
}

/// Picks skin `id` for every window.
export function setSkin(id: string) {
  try { localStorage.setItem(KEY, id); } catch {}
  apply();
}

/// Draws this window in the picked skin, and follows picks made in others and skin
/// plugins turned on, off or changed.
export function followSkin() {
  apply();
  addEventListener("storage", e => { if (e.key === KEY) apply(); });

  // The payload is the list, or null while the desktop has not started its plugins.
  void listen<unknown>("plugins-status", ({ payload }) => { if (payload !== null) takePluginSkins(payload, ++skinReads); }).catch(() => {});
  void readPluginSkins();
}

let skinReads = 0;

async function readPluginSkins() {
  const mine = ++skinReads;
  takePluginSkins(await invoke<unknown>("plugins_status").catch(() => null), mine);
}

/// Takes `raw` as the plugin list, unless a newer list or read came since `mine`.
function takePluginSkins(raw: unknown, mine: number) {
  const rows = parseRows(raw);
  if (rows === null || mine !== skinReads) return;

  pluginsRead = true;
  skinPlugins.setState({ skins: pluginSkins(rows) });
  apply();
}

/// How a project's front page lays out: the board, or the sections and rows
/// before it. Picked in Settings, under Appearance.
export type FrontLayout = "board" | "list";
export type FrontPage = "project";
const layoutKey = (page: FrontPage) => `beep.layout.${page}`;
const kept = (page: FrontPage): FrontLayout => { try { return localStorage.getItem(layoutKey(page)) === "list" ? "list" : "board"; } catch { return "board"; } };
const layouts: Record<FrontPage, FrontLayout> = { project: kept("project") };
export const frontLayout = (page: FrontPage) => layouts[page];
/// Called when the user picks a layout: every page that shows it draws again.
export const layoutChanged = new Set<() => void>();

export function setFrontLayout(page: FrontPage, next: FrontLayout) {
  if (next === layouts[page]) return;
  layouts[page] = next;
  try { localStorage.setItem(layoutKey(page), next); } catch { /* kept for this launch only */ }
  for (const draw of layoutChanged) draw();
}
