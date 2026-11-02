// What the sidebar's chat rows know beyond the chat list: which chats are
// still answering, and which are chats about one pull request or issue,
// listed under their project instead of under Chats.

/// A chat about one thread of a project (ThreadChatRow), with its project.
export interface ThreadChatEntry {
  chat_id: string;
  /// `pr` or `issue`.
  kind: string;
  repo: string;
  number: number;
  title: string;
  project_id: string;
  /// The project's `org/repo`.
  project: string;
}

/// The chats with a reply coming: those this window runs, and those the
/// server says run, started on any device.
export function runningChats(local: Iterable<string | null>, server: Iterable<string> | null | undefined) {
  const out = new Set<string>();
  for (const id of local) if (id) out.add(id);
  for (const id of server ?? []) out.add(id);
  return out;
}

/// "PR #41" or "Issue #12".
export function threadLabel(kind: string, number: number) {
  return `${kind === "pr" ? "PR" : "Issue"} #${number}`;
}

/// What a link to a thread opens on: "Open on GitHub" for github.com,
/// else "Open in browser".
export function openLabel(url: string) {
  try {
    const host = new URL(url).hostname.toLowerCase();
    if (host === "github.com" || host.endsWith(".github.com")) return "Open on GitHub";
  } catch {}
  return "Open in browser";
}

/// The thread chats each project lists under it in the sidebar, by
/// project name: the open one and the running ones always, then the most
/// recent, up to `limit` in all unless more are open or running. `recent`
/// is the chat list's ids, newest first; a thread chat not in it comes
/// after those that are.
export function sideThreads(
  chats: Iterable<ThreadChatEntry>,
  recent: readonly string[],
  open: string | null,
  running: ReadonlySet<string>,
  limit: number,
) {
  const rank = new Map(recent.map((id, i) => [id, i]));
  const byProject = new Map<string, ThreadChatEntry[]>();
  for (const c of chats) {
    let list = byProject.get(c.project);
    if (!list) byProject.set(c.project, list = []);
    list.push(c);
  }
  const at = (c: ThreadChatEntry) => rank.get(c.chat_id) ?? Number.MAX_SAFE_INTEGER;
  const out = new Map<string, ThreadChatEntry[]>();
  for (const [project, list] of byProject) {
    const sorted = [...list].sort((a, b) => at(a) - at(b));
    const must = (c: ThreadChatEntry) => c.chat_id === open || running.has(c.chat_id);
    let room = Math.max(0, limit - sorted.filter(must).length);
    const kept = sorted.filter(c => {
      if (must(c)) return true;
      if (room <= 0) return false;
      room--;
      return true;
    });
    if (kept.length) out.set(project, kept);
  }
  return out;
}
