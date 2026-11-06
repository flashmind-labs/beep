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
export function logError(message: string) {
  core.invoke("log_webview", { message }).catch(() => {});
}

/// Calls a Rust command, logging it when it fails. With `within` (ms) a
/// command that has not answered by then fails with a plain error, so a
/// call that never returns cannot hold the page.
export function invoke<T = unknown>(cmd: string, args?: Record<string, unknown>, within?: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const call = core.invoke<T>(cmd, args);
  const limited = within == null ? call : Promise.race([call, new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(`no answer in ${Math.round(within / 1000)} s. Try again.`), within);
  })]);
  return limited.finally(() => clearTimeout(timer)).catch((e: unknown) => {
    const session = typeof args?.session === "string" ? ` (session ${args.session})` : "";
    logError(`${cmd} failed${session}: ${typeof e === "object" && e && !(e instanceof Error) ? JSON.stringify(e) : e}`);
    throw e;
  });
}

window.addEventListener("error", e => logError(e.error instanceof Error ? e.error.stack ?? String(e.error) : e.message));
window.addEventListener("unhandledrejection", e => logError(`unhandled rejection: ${e.reason instanceof Error ? e.reason.stack ?? e.reason : e.reason}`));
export const { listen } = window.__TAURI__.event;

