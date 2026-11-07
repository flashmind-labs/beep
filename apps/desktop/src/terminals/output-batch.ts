// Shells can echo a key and redraw their suggestions in separate writes.
// Give those writes a short window to reach xterm together.
const WINDOW_MS = 12;
const MAX_BYTES = 64 * 1024;

export function outputBatch(write: (data: Uint8Array | string) => void) {
  let pending: (Uint8Array | string)[] = [];
  let bytes = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  function flush() {
    clearTimeout(timer);
    timer = undefined;
    const batch = pending;
    pending = [];
    bytes = 0;
    for (const data of batch) write(data);
  }
  return {
    write(data: Uint8Array | string) {
      pending.push(data);
      bytes += data.length;
      if (bytes >= MAX_BYTES) flush();
      else if (timer === undefined) timer = setTimeout(flush, WINDOW_MS);
    },
    dispose() {
      clearTimeout(timer);
      timer = undefined;
      pending = [];
      bytes = 0;
    },
  };
}
