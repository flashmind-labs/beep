// The clipboard, and saying a copy happened.
/// `text` on the clipboard. The webview may refuse the async way; the old way
/// still works.
export async function writeClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const previous = document.activeElement;
    const t = document.createElement("textarea");
    t.value = text;
    document.body.append(t);
    try {
      t.select();
      if (!document.execCommand("copy")) throw new Error("Copy failed");
    } finally {
      t.remove();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    }
  }
}

/// Copies `text` and says so on `on` for a moment.
export async function copy(text: string, on?: HTMLElement | null) {
  await writeClipboard(text);
  if (!on) return;
  const was = on.innerHTML;
  on.textContent = "Copied";
  setTimeout(() => { if (on.isConnected) on.innerHTML = was; }, 1200);
}
