// HTML files offer a rendered preview beside their source.
export function isHtmlFile(path: string): boolean {
  return /\.html?$/i.test(path);
}

const pending = new WeakSet<object>();
export function queuePreview(tab: object) { pending.add(tab); }
export function takeQueuedPreview(tab: object): boolean {
  const found = pending.has(tab);
  pending.delete(tab);
  return found;
}
