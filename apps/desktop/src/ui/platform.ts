export const WINDOWS = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
/// macOS, iOS and iPadOS, where Command is the app modifier.
export const APPLE = typeof navigator !== "undefined" && /Mac|iPhone|iPad|iPod/.test(navigator.userAgent);

/// The app modifier: Control on Windows, Command on macOS.
export function commandKey(e: { metaKey: boolean; ctrlKey?: boolean }, windows = WINDOWS): boolean {
  return windows ? !!e.ctrlKey : e.metaKey;
}

/// Windows narrower than this show the phone layout.
const NARROW_PX = 700;
/// A touch screen up to this wide does too (a tablet keeps the desktop layout).
const TOUCH_PX = 900;
/// A phone whose shorter side is at least this wide, such as an unfolded
/// foldable, shows the desktop layout (Android's large screen width).
const WIDE_PHONE_PX = 600;

interface LayoutEnv {
  width: number;
  height: number;
  /// `(pointer: coarse)`: the main pointer is a finger.
  coarse: boolean;
  /// The app runs on a phone: Tauri mobile.
  phone: boolean;
}

/// Whether the center window shows the phone layout: on a phone's small screen,
/// in a narrow window, or on a small touch screen. Landscape does not make a
/// phone wide, its shorter side decides.
export function mobileLayout(env: LayoutEnv): boolean {
  if (env.phone) return Math.min(env.width, env.height) < WIDE_PHONE_PX;

  return env.width < NARROW_PX || (env.coarse && env.width < TOUCH_PX);
}

/// Whether the main pointer is a finger, as on a phone or tablet.
export function isTouchDevice(): boolean {
  return typeof matchMedia === "function" && matchMedia("(pointer: coarse)").matches;
}

/// The app runs on a phone (Tauri mobile). The Rust side may set
/// `window.__BEEP_PHONE__` before the page loads; otherwise the user agent says.
export function onPhone(): boolean {
  if (typeof window === "undefined") return false;
  const flag = (window as { __BEEP_PHONE__?: boolean }).__BEEP_PHONE__;
  if (flag !== undefined) return flag;
  return /Android|iPhone|iPad|iPod/.test(navigator.userAgent);
}

/// The phone layout applies now. Desktop-only things (project windows, zoom,
/// autostart, local editors and folders, Mac control) check `onPhone()`, which
/// does not change with the window's width.
export function isMobile(): boolean {
  if (typeof window === "undefined") return false;
  return mobileLayout({ width: window.innerWidth, height: window.innerHeight, coarse: window.matchMedia("(pointer: coarse)").matches, phone: onPhone() });
}

/// iPhone and iPad, where saves go to the app's folder in Files.
function onIos(): boolean {
  return APPLE && onPhone();
}

/// The folder a save at `path` went to, as the user knows it: Files on
/// iOS, otherwise the folder's own name.
export function savedIn(path: string): string {
  if (onIos()) return "Files";
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.at(-2) ?? "Downloads";
}

/// The button after a save: Finder on a computer, the share sheet on iOS.
export function revealLabel(): "Share" | "Show in Finder" {
  return onIos() ? "Share" : "Show in Finder";
}
