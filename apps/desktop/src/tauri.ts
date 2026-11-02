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

export const { invoke, Channel } = window.__TAURI__.core;
export const { listen } = window.__TAURI__.event;

