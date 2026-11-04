// Tauri's global API (`withGlobalTauri`), typed for the pages.

/// A channel Rust streams events into, passed as an `invoke` argument.
export interface TauriChannel<T> {
  onmessage: (message: T) => void;
}

/// An event Rust emits to the webviews.
export interface TauriEvent<T> {
  payload: T;
}

interface TauriGlobal {
  core: {
    invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T>;
    Channel: new <T = unknown>() => TauriChannel<T>;
  };
  event: {
    listen<T = unknown>(event: string, handler: (event: TauriEvent<T>) => void): Promise<() => void>;
  };
}

declare global {
  interface Window {
    __TAURI__: TauriGlobal;
  }
}

const core = window.__TAURI__.core;
export const { Channel } = core;

/// Sends a page's error to the app's log.
function logError(message: string) {
  core.invoke("log_webview", { message }).catch(() => {});
}

/// Calls a Rust command, logging it when it fails.
export function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return core.invoke<T>(cmd, args).catch((e: unknown) => {
    logError(`${cmd} failed: ${e}`);
    throw e;
  });
}

window.addEventListener("error", e => logError(e.error instanceof Error ? e.error.stack ?? String(e.error) : e.message));
window.addEventListener("unhandledrejection", e => logError(`unhandled rejection: ${e.reason instanceof Error ? e.reason.stack ?? e.reason : e.reason}`));
export const { listen } = window.__TAURI__.event;

