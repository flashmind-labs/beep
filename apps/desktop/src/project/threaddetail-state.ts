// The project page's pane for one pull request or issue: its state, labels,
// description and comments, read from the host through a machine with the
// project. The app renders the text as Markdown and drops raw
// HTML before it gets here, so the HTML it gives is safe to insert. Commenting stays on the host.
import { invoke } from "../ui/tauri.ts";
import { reason } from "../ui/format.ts";

/// One comment, as cleaned HTML (PostHtmlRow).
interface PostRow {
  author: string;
  /// In milliseconds since the Unix epoch.
  at: number;
  html: string;
}

/// A pull request or issue read in full (ThreadDetailRow).
export interface ThreadDetailRow {
  /// `pr` or `issue`.
  kind: string;
  number: number;
  title: string;
  url: string;
  author: string;
  state: "open" | "closed" | "merged";
  draft: boolean;
  labels: string[];
  /// In milliseconds since the Unix epoch.
  opened_at: number;
  body_html: string;
  comments: PostRow[];
  /// Older comments left out.
  earlier: number;
  review: "none" | "approved" | "changes_requested";
  checks: string | null;
  /// For an issue, the pull requests that will close it or mention it.
  linked_prs: LinkedPrRow[];
}

/// A pull request linked to an issue (LinkedPrRow).
export interface LinkedPrRow {
  number: number;
  title: string;
  url: string;
  state: ThreadDetailRow["state"];
}

/// What the list row already knows, shown while the full read is on its way.
export interface ThreadStub {
  number: number;
  title: string;
  url: string;
  author: string;
  opened_at: number;
}

/// A read: null when no machine with the project could give one.
type Read = { data?: ThreadDetailRow | null; error?: string; at: number; reading: boolean };

/// How long a read stays good before the pane reads it again.
const STALE_MS = 60_000;

/// A comment being written or posted under one thread.
type Draft = { text: string; posting: boolean; error?: string };

/// One thread as its pane shows it: the read, the comment being written, and how to change it.
export interface ThreadModel {
  key: string;
  stub: ThreadStub;
  data: ThreadDetailRow | null | undefined;
  error: string | undefined;
  /// The comment being written as it stands now, which changes while the pane stays.
  writing(): { text: string; posting: boolean; draftError: string | undefined };
  edit(value: string): void;
  post(): void;
}

/// The threads read in full and the comments written under them.
export interface ThreadDetails {
  thread(projectId: string, stub: ThreadStub): ThreadModel;
}

/// A comment goes to the host through a machine with the project.
export function makeThreadDetail(deps: { redraw: () => void }): ThreadDetails {
  const reads = new Map<string, Read>();
  const drafts = new Map<string, Draft>();

  function read(projectId: string, number: number) {
    const key = `${projectId}#${number}`;
    const had = reads.get(key);
    if (had && (had.reading || Date.now() - had.at < STALE_MS)) return had;
    const r: Read = { ...had, at: Date.now(), reading: true };
    reads.set(key, r);
    invoke<ThreadDetailRow | null>("center_thread_detail", { projectId, number })
      .then(data => { r.data = data; r.error = undefined; })
      .catch(e => { r.error = reason(e); })
      .finally(() => { r.reading = false; deps.redraw(); });
    return r;
  }

  /// One thread as its pane shows it: the read, the comment being written, and how to change it.
  /// Its key is the thread, so typing a comment redraws only the pane.
  function thread(projectId: string, stub: ThreadStub) {
    const key = `${projectId}#${stub.number}`, readState = read(projectId, stub.number), data = readState.data;
    const draft = drafts.get(key) ?? { text: "", posting: false };
    drafts.set(key, draft);
    const post = () => {
      const body = draft.text.trim();
      if (!data || !body || draft.posting) return;
      draft.posting = true; draft.error = undefined; deps.redraw();
      invoke("center_comment_thread", { projectId, kind: data.kind, number: data.number, body })
        .then(() => { draft.text = ""; const r = reads.get(key); if (r) r.at = 0; })
        .catch(e => { draft.error = reason(e); })
        .finally(() => { draft.posting = false; deps.redraw(); });
    };

    /// The comment being written as it stands now, which changes while the pane stays.
    const writing = () => ({ text: draft.text, posting: draft.posting, draftError: draft.error });

    return { key, stub, data, error: readState.error, writing, edit: (value: string) => { draft.text = value; }, post };
  }

  return { thread };
}
