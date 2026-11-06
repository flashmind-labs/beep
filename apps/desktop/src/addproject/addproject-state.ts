// What the "Add a project" sheet decides, as pure functions so they can be
// tested without Tauri: which found folders a search shows, which are
// ticked, and running a batch of imports a few at a time.
import { agentLabel } from "../project/projects-state.ts";
import type { FoundRow } from "../project/projects-state.ts";

/// At most this many imports run at once in a batch.
export const IMPORTS_AT_ONCE = 3;

/// Where one found folder's import stands.
export type ImportState = { state: "importing" } | { state: "done" } | { state: "failed"; why: string };

/// True when every word of `query` is in the row's repo, remote, a folder,
/// a machine or an agent, ignoring case. An empty query shows every row.
export function foundMatches(row: Pick<FoundRow, "name" | "remote" | "places" | "agents">, query: string) {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return true;
  const texts = [
    row.name,
    row.remote,
    ...row.places.flatMap(p => [p.folder, p.machine_name]),
    ...row.agents.flatMap(a => [a.agent, agentLabel(a.agent)]),
  ].map(t => String(t || "").toLowerCase());
  return words.every(w => texts.some(t => t.includes(w)));
}

/// A row can be ticked unless it is importing or already in.
export function tickable(status: ImportState | undefined) {
  return !status || status.state === "failed";
}

/// The shown rows a batch imports: ticked and tickable, in list order.
export function toImport(shown: readonly string[], picked: ReadonlySet<string>, status: ReadonlyMap<string, ImportState>) {
  return shown.filter(name => picked.has(name) && tickable(status.get(name)));
}

/// "Select all shown": ticks every shown tickable row, or unticks them all
/// when they are all ticked already. Rows the search hides keep their tick.
export function toggleAllShown(shown: readonly string[], picked: ReadonlySet<string>, status: ReadonlyMap<string, ImportState>) {
  const open = shown.filter(name => tickable(status.get(name)));
  const all = open.length > 0 && open.every(name => picked.has(name));
  const out = new Set(picked);
  for (const name of open) {
    if (all) out.delete(name);
    else out.add(name);
  }
  return out;
}

/// Runs `run` on every item, at most `limit` at once, in order. A rejection
/// is logged and the rest keep going; `run` reports its own failures.
export async function eachLimited<T>(items: readonly T[], limit: number, run: (item: T) => Promise<void>) {
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const item = items[next++];
      try {
        await run(item);
      } catch (e) {
        console.warn("an import in a batch failed:", e);
      }
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, worker));
}

/// The machines still looking for checkouts again, as one line; null when none.
export function lookingOn(names: readonly string[]) {
  return names.length ? `Looking on ${names.join(", ")}…` : null;
}

/// A machine that could not say what it has, named in the sheet.
export function unreadText(machine: string, why: string) {
  return `Not read on ${machine}: ${why.trim().replace(/\.$/, "")}.`;
}

/// What the found list says with no rows: nothing while machines still look,
/// the machines that could not say, else the error or "No other folders yet."
export function emptyText(looking: readonly string[], unread: readonly string[], error: string | null) {
  if (looking.length) return null;
  if (unread.length) return unread.join(" ");
  return error || "No other folders yet.";
}
