export const WINDOWS = typeof navigator !== "undefined" && navigator.userAgent.includes("Windows");
/// macOS, iOS and iPadOS, where Command is the app modifier.
export const APPLE = typeof navigator !== "undefined" && /Mac|iPhone|iPad|iPod/.test(navigator.userAgent);

/// The app modifier: Control on Windows, Command on macOS.
export function commandKey(e: { metaKey: boolean; ctrlKey?: boolean }, windows = WINDOWS): boolean {
  return windows ? !!e.ctrlKey : e.metaKey;
}

/// Windows narrower than this show the phone layout.
export const NARROW_PX = 700;
/// A touch screen up to this wide does too (a tablet keeps the desktop layout).
const TOUCH_PX = 900;

export interface LayoutEnv {
  width: number;
  /// `(pointer: coarse)`: the main pointer is a finger.
  coarse: boolean;
  /// The app runs on a phone: Tauri mobile.
  phone: boolean;
}

/// Whether the center window shows the phone layout: on a phone, in a narrow
/// window, or on a small touch screen.
export function mobileLayout(env: LayoutEnv): boolean {
  return env.phone || env.width < NARROW_PX || (env.coarse && env.width < TOUCH_PX);
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
  return mobileLayout({ width: window.innerWidth, coarse: window.matchMedia("(pointer: coarse)").matches, phone: onPhone() });
}
