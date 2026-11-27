import test from "node:test";
import assert from "node:assert/strict";
import { nativeEvent } from "./bar-env.ts";

const storage = new Map<string, string>();
const styles = new Map<string, string>();
const dataset: Record<string, string | undefined> = {};
let prefersLight = false;
const mediaListeners = new Set<(event: { matches: boolean }) => void>();
const themeCalls: ("light" | "dark" | null)[] = [];
let reportedNativeTheme: "light" | "dark" = "dark";

const g = globalThis as Record<string, unknown>;
Object.defineProperty(globalThis, "navigator", {
  value: { platform: "Linux x86_64", userAgent: "Mozilla/5.0 (X11; Linux x86_64)", maxTouchPoints: 0 },
  configurable: true,
});
g.localStorage = {
  getItem: (key: string) => storage.get(key) ?? null,
  setItem: (key: string, value: string) => { storage.set(key, value); },
  removeItem: (key: string) => { storage.delete(key); },
  clear: () => { storage.clear(); },
};
Object.assign(g.document as Record<string, unknown>, {
  documentElement: {
    dataset,
    style: {
      setProperty: (key: string, value: string) => { styles.set(key, value); },
      removeProperty: (key: string) => { styles.delete(key); },
    },
  },
});
g.matchMedia = (query: string) => ({
  get matches() { return query === "(prefers-color-scheme: light)" && prefersLight; },
  media: query,
  addEventListener: (_name: string, listener: (event: { matches: boolean }) => void) => {
    if (query === "(prefers-color-scheme: light)") mediaListeners.add(listener);
  },
  removeEventListener: (_name: string, listener: (event: { matches: boolean }) => void) => {
    mediaListeners.delete(listener);
  },
});
const tauri = g.__TAURI__ as { webviewWindow: { getCurrentWebviewWindow(): Record<string, unknown> } };
const baseWindow = tauri.webviewWindow.getCurrentWebviewWindow();
tauri.webviewWindow.getCurrentWebviewWindow = () => ({
  ...baseWindow,
  label: "center",
  setTheme: async (theme: "light" | "dark" | null) => { themeCalls.push(theme); },
  theme: async () => reportedNativeTheme,
});

const {
  darkSkinId,
  followSkin,
  lightSkinId,
  setSkin,
  setSystemSkin,
  skinId,
  skinPlugins,
  skinPreference,
  systemSkin,
} = await import("../src/ui/skin.ts");

followSkin();

test("system theme switches between chosen dark and light skins and syncs native window theme", async () => {
  assert.equal(systemSkin(), false);
  assert.equal(skinId(), "linear");
  assert.equal(dataset.skin, undefined);
  assert.deepEqual(themeCalls, ["dark"]);

  setSkin("nord");
  setSkin("warm");
  assert.equal(darkSkinId(), "nord");
  assert.equal(lightSkinId(), "warm");
  assert.equal(dataset.skin, "warm");
  assert.deepEqual(themeCalls, ["dark", "light"]);

  reportedNativeTheme = "dark";
  setSystemSkin(true);
  await new Promise(resolve => setImmediate(resolve));
  assert.equal(systemSkin(), true);
  assert.equal(themeCalls.at(-1), null);
  assert.equal(skinId(), "nord");
  assert.equal(dataset.skin, "nord");
  assert.deepEqual(skinPreference.getState(), { system: true, active: "nord", dark: "nord", light: "warm" });

  // Native portal or OS theme flip to light updates the active skin without overriding system mode.
  const countBeforeFlip = themeCalls.length;
  nativeEvent("tauri://theme-changed", "light");
  assert.equal(skinId(), "warm");
  assert.equal(dataset.skin, "warm");
  assert.equal(themeCalls.length, countBeforeFlip);

  // Picking a dark or light skin while in system mode updates that side and keeps system mode on.
  setSkin("tokyo");
  assert.equal(systemSkin(), true);
  assert.equal(darkSkinId(), "tokyo");
  assert.equal(dataset.skin, "warm");

  setSkin("solarized");
  assert.equal(systemSkin(), true);
  assert.equal(lightSkinId(), "solarized");
  assert.equal(dataset.skin, "solarized");

  // Media query change back to dark switches to the updated dark skin.
  prefersLight = false;
  for (const listener of mediaListeners) listener({ matches: false });
  assert.equal(skinId(), "tokyo");
  assert.equal(dataset.skin, "tokyo");

  // Turning system mode off keeps the skin currently shown.
  setSystemSkin(false);
  assert.equal(systemSkin(), false);
  assert.equal(skinId(), "tokyo");
  assert.equal(dataset.skin, "tokyo");
  assert.equal(themeCalls.at(-1), "dark");

  // Picking a plugin skin while in system mode leaves system mode and applies the plugin tokens.
  setSystemSkin(true);
  await new Promise(resolve => setImmediate(resolve));
  skinPlugins.setState({ skins: [{ id: "flat", name: "Flat", swatch: "#202020", tokens: new Map([["--radius", "0"]]) }] });
  setSkin("plugin:flat");
  assert.equal(systemSkin(), false);
  assert.equal(skinId(), "plugin:flat");
  assert.equal(styles.get("--radius"), "0");
});
