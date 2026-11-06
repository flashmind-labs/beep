// ⌘N with a terminal on screen: a new terminal like it, not a new agent.

/// A terminal on screen: where it runs, and whether it has the keys.
export interface TermShown {
  machine_id: string;
  machine_name: string;
  cwd: string;
  project: string;
  focused: boolean;
}

/// ⌘N and no other modifier.
export function isNewKey(e: Pick<KeyboardEvent, "key" | "metaKey" | "shiftKey" | "altKey" | "ctrlKey">) {
  return e.metaKey && !e.shiftKey && !e.altKey && !e.ctrlKey && e.key.toLowerCase() === "n";
}

/// The terminal ⌘N starts another like: the focused one, else the first on
/// screen. A terminal with no machine is skipped. Null when none shows, and
/// ⌘N starts an agent as anywhere else.
export function newTermLike<T extends TermShown>(shown: readonly T[]): T | null {
  const ok = shown.filter(t => t.machine_id);
  return ok.find(t => t.focused) ?? ok[0] ?? null;
}
