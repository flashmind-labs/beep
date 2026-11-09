// The project page's pane for one pull request or issue: its state, labels,
// description and comments, read from the host through a machine with the
// project, or the server. The app renders the text as Markdown and drops raw
// HTML before it gets here, so the HTML it gives is safe to insert. Commenting stays on the host.
import { createElement } from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { el as h, type Kid } from "../ui/dom.ts";
import { invoke } from "../ui/tauri.ts";
import { ThreadDetailView } from "./threaddetail-view.tsx";

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
}

/// What the list row already knows, shown while the full read is on its way.
export interface ThreadStub {
  number: number;
  title: string;
  url: string;
  author: string;
  opened_at: number;
}

/// A read: null from a server too old to give one.
type Read = { data?: ThreadDetailRow | null; error?: string; at: number; reading: boolean };

/// How long a read stays good before the pane reads it again.
const STALE_MS = 60_000;

/// "Open on GitHub", else "Open on Forgejo": a project's host is one or the other.
export function forgeLabel(url: string) {
  try { return new URL(url).host === "github.com" ? "Open on GitHub" : "Open on Forgejo"; } catch { return "Open on GitHub"; }
}

const slots = new WeakMap<Element, HTMLElement>();
export function threadPanel(slot: Element) { return slots.get(slot) ?? null; }

/// A comment being written or posted under one thread.
type Draft = { text: string; posting: boolean; error?: string };

/// `comment()` says whether the pane offers a comment box: with no server,
/// the comment goes to the host through a machine with the project.
export function makeThreadDetail(deps: { redraw: () => void; openUrl: (url: string) => unknown; comment: () => boolean }) {
  const reads = new Map<string, Read>();
  const drafts = new Map<string, Draft>();
  let current: { key: string; host: HTMLElement; root: Root } | null = null;

  function read(projectId: string, number: number) {
    const key = `${projectId}#${number}`;
    const had = reads.get(key);
    if (had && (had.reading || Date.now() - had.at < STALE_MS)) return had;
    const r: Read = { ...had, at: Date.now(), reading: true };
    reads.set(key, r);
    invoke<ThreadDetailRow | null>("center_thread_detail", { projectId, number })
      .then(data => { r.data = data; r.error = undefined; })
      .catch(e => { r.error = String(e).replace(/^(bad request|error):\s*/i, ""); })
      .finally(() => { r.reading = false; deps.redraw(); });
    return r;
  }

  function pane(projectId: string, stub: ThreadStub, acts: Kid[]): Node[] {
    const key = `${projectId}#${stub.number}`, readState = read(projectId, stub.number), data = readState.data;
    const draft = drafts.get(key) ?? { text: "", posting: false };
    drafts.set(key, draft);
    if (current?.key !== key) {
      current?.root.unmount();
      const host = h("div"); host.style.display = "contents";
      current = { key, host, root: createRoot(host) };
    }
    const panel = current, actions = h("div", "", ...acts);
    const post = () => {
      const body = draft.text.trim();
      if (!data || !body || draft.posting) return;
      draft.posting = true; draft.error = undefined; deps.redraw();
      invoke("center_comment_thread", { projectId, kind: data.kind, number: data.number, body })
        .then(() => { draft.text = ""; const r = reads.get(key); if (r) r.at = 0; })
        .catch(e => { draft.error = String(e).replace(/^(bad request|error):\s*/i, ""); })
        .finally(() => { draft.posting = false; deps.redraw(); });
    };
    const draw = () => flushSync(() => panel.root.render(createElement(ThreadDetailView, {
      stub, data, error: readState.error, acts: actions, comment: deps.comment(), text: draft.text, posting: draft.posting, draftError: draft.error,
      edit: (value: string) => { draft.text = value; draw(); }, post, open: (url: string) => { void deps.openUrl(url); },
    })));
    draw();
    const slot = h("div"); slots.set(slot, panel.host);
    return [slot];
  }

  return { pane };
}
