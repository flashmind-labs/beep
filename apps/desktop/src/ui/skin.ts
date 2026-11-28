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
const DEFAULT_LIGHT_SKIN = "paper";
const KEY = "beep.skin";
const SYSTEM_KEY = "beep.skin.system";
const DARK_KEY = "beep.skin.dark";
const LIGHT_KEY = "beep.skin.light";
const SKIN_KEYS = new Set([KEY, SYSTEM_KEY, DARK_KEY, LIGHT_KEY]);
/// The picked plugin skin's tokens, so a window shows them before the plugins are read.
const TOKENS_KEY = "beep.skin.tokens";
/// A plugin skin's id in the pick.
const PLUGIN = "plugin:";

interface SkinPreference {
  /// Whether the window switches between `dark` and `light` with the OS preference.
  system: boolean;
  /// The skin id drawn right now.
  active: string;
  /// The dark skin used when `system` is on.
  dark: string;
  /// The light skin used when `system` is on.
  light: string;
}

/// The skins installed plugins offer, read from the desktop. Empty on a phone.
export const skinPlugins = createStore<{ skins: readonly PluginSkin[] }>(() => ({ skins: [] }));
let pluginsRead = false;
const memoryStore = new Map<string, string>();
/// What Windows or Linux reported for the OS theme while following the system;
/// null when `prefers-color-scheme` speaks for itself.
let nativeLight: boolean | null = null;

export const pluginSkinId = (plugin: string) => `${PLUGIN}${plugin}`;

const isDarkBuiltin = (id: string | null): id is string => id !== null && SKINS.some(s => s.id === id && !s.light);
const isLightBuiltin = (id: string | null): id is string => id !== null && SKINS.some(s => s.id === id && s.light === true);

function readItem(key: string): string | null {
  try { return localStorage.getItem(key); } catch { return memoryStore.get(key) ?? null; }
}

function writeItem(key: string, value: string) {
  memoryStore.set(key, value);
  try { localStorage.setItem(key, value); } catch {}
}

/// Whether the window follows the system's light or dark appearance.
export function systemSkin(): boolean {
  return readItem(SYSTEM_KEY) === "true";
}

/// The dark skin paired with system mode: the one picked for dark, else a dark
/// fixed pick, else Linear.
export function darkSkinId(): string {
  const dark = readItem(DARK_KEY);
  if (isDarkBuiltin(dark)) return dark;
  const fixed = readItem(KEY);
  return isDarkBuiltin(fixed) ? fixed : DEFAULT_SKIN;
}

/// The light skin paired with system mode: the one picked for light, else a
/// light fixed pick, else Light.
export function lightSkinId(): string {
  const light = readItem(LIGHT_KEY);
  if (isLightBuiltin(light)) return light;
  const fixed = readItem(KEY);
  return isLightBuiltin(fixed) ? fixed : DEFAULT_LIGHT_SKIN;
}

function systemPrefersLight(): boolean {
  if (nativeLight !== null) return nativeLight;
  return typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: light)").matches;
}

function fixedSkinId(): string {
  const kept = readItem(KEY);
  if (kept === null) return DEFAULT_SKIN;
  if (kept.startsWith(PLUGIN)) return !pluginsRead || skinPlugins.getState().skins.some(s => pluginSkinId(s.id) === kept) ? kept : DEFAULT_SKIN;
  return SKINS.some(s => s.id === kept) ? kept : DEFAULT_SKIN;
}

/// The picked skin's id, following the system when enabled or falling back to Linear.
export function skinId(): string {
  return systemSkin() ? (systemPrefersLight() ? lightSkinId() : darkSkinId()) : fixedSkinId();
}

function readSkinPreference(): SkinPreference {
  return { system: systemSkin(), active: skinId(), dark: darkSkinId(), light: lightSkinId() };
}

export const skinPreference = createStore<SkinPreference>(() => readSkinPreference());

/// Called when this window takes another skin: what draws in its colours
/// outside CSS, such as a terminal, draws again.
export const skinChanged = new Set<() => void>();

/// The tokens this window set from a plugin skin, to clear them on the next pick.
let pluginTokens: readonly string[] = [];
type WindowTheme = "light" | "dark" | "system";
let appliedWindowTheme: WindowTheme | null = null;
let themeSync = 0;

function syncWindowTheme(system: boolean, builtin: string) {
  if (APPLE || onPhone()) return;

  const desired: WindowTheme = system ? "system" : (isLightBuiltin(builtin) ? "light" : "dark");
  if (desired === appliedWindowTheme) return;
  appliedWindowTheme = desired;
  const mine = ++themeSync;
  const webview = window.__TAURI__.webviewWindow.getCurrentWebviewWindow();
  if (desired !== "system") {
    nativeLight = null;
    webview.setTheme?.(desired).catch(() => {});
    return;
  }

  if (!webview.setTheme) return;
  webview.setTheme(null)
    .then(() => webview.theme?.())
    .then(theme => {
      if (mine !== themeSync || !systemSkin() || (theme !== "light" && theme !== "dark")) return;
      const isLight = theme === "light";
      if (nativeLight === isLight) return;
      nativeLight = isLight;
      apply();
    })
    .catch(() => {});
}

function apply() {
  const system = systemSkin();
  const id = skinId();
  const builtin = id.startsWith(PLUGIN) ? DEFAULT_SKIN : id;
  if (builtin === DEFAULT_SKIN) delete document.documentElement.dataset.skin;
  else document.documentElement.dataset.skin = builtin;

  const tokens = id.startsWith(PLUGIN) ? pluginTokensOf(id) : new Map<string, string>();
  const root = document.documentElement.style;
  for (const token of pluginTokens) if (!tokens.has(token)) root.removeProperty(token);
  for (const [token, value] of tokens) root.setProperty(token, value);
  pluginTokens = [...tokens.keys()];
  skinPreference.setState(readSkinPreference());
  for (const draw of skinChanged) draw();

  // Windows and Linux draw the title bar: it follows the system or takes the skin's light or dark.
  syncWindowTheme(system, builtin);
}

/// A plugin skin's tokens: from the plugins once read, and kept for the next launch.
function pluginTokensOf(id: string): Map<string, string> {
  const skin = skinPlugins.getState().skins.find(s => pluginSkinId(s.id) === id);
  if (skin !== undefined) {
    writeItem(TOKENS_KEY, JSON.stringify([...skin.tokens]));
    return new Map(skin.tokens);
  }

  try {
    const kept: unknown = JSON.parse(readItem(TOKENS_KEY) ?? "[]");
    if (Array.isArray(kept)) return new Map(kept.filter((pair): pair is [string, string] => Array.isArray(pair) && typeof pair[0] === "string" && pair[0].startsWith("--") && typeof pair[1] === "string"));
  } catch {}
  return new Map();
}

/// Turns following the system's light or dark appearance on or off for every window.
export function setSystemSkin(enabled: boolean) {
  if (enabled) {
    writeItem(DARK_KEY, darkSkinId());
    writeItem(LIGHT_KEY, lightSkinId());
    writeItem(SYSTEM_KEY, "true");
  } else {
    writeItem(KEY, skinId());
    writeItem(SYSTEM_KEY, "false");
  }
  apply();
}

/// Picks skin `id` for every window. While following the system, a dark or light
/// built-in skin updates that side's pick; a plugin skin switches to itself.
export function setSkin(id: string) {
  if (systemSkin()) {
    if (isLightBuiltin(id)) writeItem(LIGHT_KEY, id);
    else if (isDarkBuiltin(id)) writeItem(DARK_KEY, id);
    else {
      writeItem(KEY, id);
      writeItem(SYSTEM_KEY, "false");
    }
  } else {
    writeItem(KEY, id);
    if (isLightBuiltin(id)) writeItem(LIGHT_KEY, id);
    else if (isDarkBuiltin(id)) writeItem(DARK_KEY, id);
  }
  apply();
}

/// Draws this window in the picked skin, and follows picks made in others, system
/// theme changes, and skin plugins turned on, off or changed.
export function followSkin() {
  apply();
  addEventListener("storage", e => { if (e.key === null || SKIN_KEYS.has(e.key)) apply(); });
  if (typeof matchMedia === "function") {
    matchMedia("(prefers-color-scheme: light)").addEventListener("change", e => {
      if (nativeLight !== null) nativeLight = e.matches;
      if (systemSkin()) apply();
    });
  }
  if (!APPLE && !onPhone()) {
    void window.__TAURI__.webviewWindow.getCurrentWebviewWindow()
      .listen<unknown>("tauri://theme-changed", ({ payload }) => {
        if (!systemSkin() || (payload !== "light" && payload !== "dark")) return;
        const isLight = payload === "light";
        if (nativeLight === isLight) return;
        nativeLight = isLight;
        apply();
      })
      .catch(() => {});
  }

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
