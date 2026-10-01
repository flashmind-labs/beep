// Incremental UTF-8 decoding preserves characters and JSON across arbitrary chunks.
export async function readEvents(body, onEvent) {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let pending = '';
  try {
    while (true) {
      const { value, done } = await reader.read();
      pending += done ? decoder.decode() : decoder.decode(value, { stream: true });
      let end;
      while ((end = pending.indexOf('\n')) >= 0) {
        const line = pending.slice(0, end).trim();
        pending = pending.slice(end + 1);
        if (line) onEvent(JSON.parse(line));
      }
      if (pending.length > 2 * 1024 * 1024) throw new Error('Server event exceeds size limit.');
      if (done) break;
    }
    if (pending.trim()) onEvent(JSON.parse(pending));
  } finally { reader.releaseLock(); }
}
export function acceptEvent(pending, event) {
  if (event.run_id !== pending.runId || !Number.isSafeInteger(event.seq) || event.seq <= pending.after) return false;
  pending.after = event.seq;
  return true;
}
export function uuid() {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 15) | 64;
  bytes[8] = (bytes[8] & 63) | 128;
  return Array.from(bytes, (b, i) => ([4, 6, 8, 10].includes(i) ? '-' : '') + b.toString(16).padStart(2, '0')).join('');
}
