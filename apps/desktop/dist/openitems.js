// Home's open PRs and issues, broken down by project: what each row says,
// apart from the DOM so it can be tested. `view` is what the `git_items`
// command gives: `{ items, hosts }`.

/// Items a project shows before "Show all".
export const ITEMS_SHOWN = 3;

/// The section's title from the hosts saved: "Open on GitHub", "Open on
/// Forgejo" or "Open on GitHub and Forgejo".
export function openTitle(hosts) {
  const names = [];
  if (hosts.some(h => !h.forgejo)) names.push("GitHub");
  if (hosts.some(h => h.forgejo)) names.push("Forgejo");
  return `Open on ${names.join(" and ") || "your git hosts"}`;
}

/// Whether Home shows the section: only once a git host is saved.
export function showsOpen(view) {
  return Array.isArray(view?.hosts) && view.hosts.length > 0;
}

/// Whether an item waits on the user: a PR asking for their review, or
/// one of theirs with changes requested or failing checks.
export function waitsOnYou(item) {
  if (item.kind !== "pull_request") return false;
  return item.review_requested || (item.authored && (item.review === "changes_requested" || item.checks_failing));
}

/// The repository's web page from an item's.
export function repoUrl(url) {
  return String(url || "").replace(/\/(pull|pulls|issues)\/\d+([/?#].*)?$/, "");
}

/// One row per repository with its counts and its items, newest first.
/// Projects with something waiting on the user come first, then by
/// latest activity. `projects` are Home's project rows, to know which are
/// imported.
export function projectsWithOpen(view, projects) {
  const byName = new Map((projects || []).filter(p => p.imported).map(p => [p.name.toLowerCase(), p]));
  const rows = new Map();
  for (const item of view?.items || []) {
    const key = `${item.host} ${item.repo}`;
    let row = rows.get(key);
    if (!row) {
      const project = byName.get(item.repo.toLowerCase()) || null;
      row = { key, repo: item.repo, host: item.host, url: repoUrl(item.url), project, prs: 0, issues: 0, waiting: 0, last: 0, items: [] };
      rows.set(key, row);
    }
    if (item.kind === "pull_request") row.prs++;
    else row.issues++;
    if (waitsOnYou(item)) row.waiting++;
    row.last = Math.max(row.last, item.updated_at);
    row.items.push(item);
  }
  for (const row of rows.values()) row.items.sort((a, b) => (waitsOnYou(b) - waitsOnYou(a)) || b.updated_at - a.updated_at);
  return [...rows.values()].sort((a, b) => (Math.sign(b.waiting) - Math.sign(a.waiting)) || b.last - a.last);
}

/// The state chips of an item, each `{ text, cls }`.
export function itemChips(item) {
  const out = [];
  if (item.kind === "pull_request") {
    if (item.review_requested) out.push({ text: "your review", cls: "wait" });
    if (item.draft) out.push({ text: "draft", cls: "" });
    if (item.review === "changes_requested") out.push({ text: "changes requested", cls: "wait" });
    else if (item.review === "approved") out.push({ text: "approved", cls: "run" });
    if (item.checks_failing) out.push({ text: "checks failing", cls: "bad" });
  } else if (item.assigned) {
    out.push({ text: "assigned", cls: "" });
  }
  return out;
}

/// What Review prefills in Start a coding agent: the prompt and, when the
/// project is imported, the folder on an online machine that has it.
export function reviewStart(item, project, machines) {
  const tool = item.host === "github.com" ? "gh" : "teacli";
  const prompt = `Review PR ${item.url}. Read the diff with ${tool}, and post your review only after I approve it.`;
  const online = new Set((machines || []).filter(m => m.online).map(m => m.id));
  const at = project?.machines?.find(m => online.has(m.id) && m.folder) || null;
  return { prompt, at: at ? { machine: at.id, cwd: at.folder } : null };
}

/// The first message of the chat Reply opens.
export function replyPrompt(item) {
  return `Help me reply to ${item.url}`;
}
