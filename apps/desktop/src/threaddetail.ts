// The project page's pane for one pull request or issue: its state, labels,
// description and comments, read from the host through the server. The app
// renders the text as Markdown and drops raw HTML before it gets here, so
// the HTML it gives is safe to insert. Commenting stays on the host.
import { ago } from "./components/format.ts";
import { dbtn as button, emptyPanel, stateChip } from "./components/parts.ts";
import { el as h, type Kid } from "./projects-state.ts";
import { invoke } from "./tauri.ts";

/// One comment, as cleaned HTML (PostHtmlRow).
interface PostRow {
  author: string;
  /// In milliseconds since the Unix epoch.
  at: number;
  html: string;
}

/// A pull request or issue read in full (ThreadDetailRow).
interface ThreadDetailRow {
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

const CHECKS: Record<string, [string, string]> = {
  failed: ["blocked", "checks failing"], passed: ["done", "checks passing"], running: ["running", "checks running"],
  queued: ["running", "checks queued"], waiting: ["running", "checks waiting"],
};

export function makeThreadDetail(deps: { redraw: () => void; openUrl: (url: string) => unknown }) {
  const reads = new Map<string, Read>();

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

  /// A cleaned HTML block.
  function md(html: string) {
    const box = h("div", "pj-md");
    box.innerHTML = html;
    for (const a of box.querySelectorAll("a")) {
      a.addEventListener("click", e => { e.preventDefault(); if (a.href) deps.openUrl(a.href); });
    }
    return box;
  }

  /// The pane: the head with `acts` and Open on the host, then the
  /// description and the comments, oldest first.
  function pane(projectId: string, stub: ThreadStub, acts: Kid[]): Node[] {
    const r = read(projectId, stub.number);
    const d = r.data;
    const meta = h("div", "meta");
    if (d) {
      meta.append(d.state === "merged" ? stateChip("done", "merged") : d.state === "closed" ? stateChip("", "closed") : stateChip("running", d.draft ? "draft" : "open"));
    }
    const author = d?.author || stub.author, opened = d?.opened_at || stub.opened_at;
    meta.append(h("span", "", `#${stub.number}`, author && " opened by ", author && h("b", "", author), opened ? ` ${ago(opened)}` : ""));
    for (const l of d?.labels || []) meta.append(h("span", "cc-tag", l));
    if (d?.review === "approved") meta.append(stateChip("done", "approved"));
    if (d?.review === "changes_requested") meta.append(stateChip("blocked", "changes requested"));
    const checks = d?.checks ? CHECKS[d.checks] : null;
    if (checks) meta.append(stateChip(checks[0], checks[1], checks[0] === "running"));
    const url = d?.url || stub.url;
    const head = h("div", "pj-dh", h("h3", "", d?.title || stub.title), meta,
      h("div", "acts", ...acts, url ? button(forgeLabel(url), "", () => deps.openUrl(url)) : null));
    const body: Node[] = [];
    if (r.error) body.push(h("div", "cc-err", r.error));
    if (d === undefined && !r.error) body.push(emptyPanel(h("span", "", "Reading it from the host…")));
    if (d === null) body.push(emptyPanel(h("span", "", "The server needs an update before it shows the description and comments.")));
    if (d) {
      body.push(d.body_html.trim() ? md(d.body_html) : h("p", "dmuted", "No description."));
      if (d.earlier) body.push(h("small", "dmuted", `${d.earlier} earlier ${d.earlier === 1 ? "comment" : "comments"} on the host`));
      for (const c of d.comments) {
        body.push(h("div", "pj-post", h("div", "by", h("b", "", c.author || "someone"), ` · ${ago(c.at)}`), md(c.html)));
      }
      body.push(h("small", "dmuted", `Read only here. Comment on ${forgeLabel(url).replace(/^Open on /, "")}.`));
    }
    return [head, h("div", "pj-db", ...body)];
  }

  return { pane };
}
