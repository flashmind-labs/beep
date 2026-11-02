// Branches with work outside the review screen: the machines a branch can
// get a worktree on, what the import sheet asks for, and the open PRs and
// issues in repos without a project. Apart from the DOM so they can be
// tested.

/// The online machines a worktree can go on, by `{ id, name }`: the one
/// the branches were read on first, then the rest by name.
export function worktreeMachines(machines, readOn) {
  const online = (machines || []).filter(m => m.online).map(m => ({ id: m.id, name: m.name }));
  online.sort((a, b) => (Number(b.id === readOn) - Number(a.id === readOn)) || a.name.localeCompare(b.name));
  return online;
}

/// ".worktrees/fix-it" from "fix/it", as the server names a worktree.
export function worktreeName(branch) {
  return `.worktrees/${String(branch).replace(/\//g, "-")}`;
}

/// Where each preview branch goes by default, by name: the first
/// checkout's machine for a branch with a pull request or commits ahead
/// and no worktree yet; nothing (Skip) for the rest. `""` stands for the
/// machine the project lands on when no checkout was found.
export function defaultPlaces(preview) {
  const out = {};
  if (!preview) return out;
  const home = preview.checkouts?.[0]?.machine_id ?? "";
  for (const b of preview.branches || []) {
    if (b.name === preview.default_branch || b.worktree) continue;
    if (b.pr != null || b.ahead > 0) out[b.name] = home;
  }
  return out;
}

/// The machines a preview branch can get its worktree on: those with a
/// checkout, or the one it is imported on when none has one.
export function placeChoices(preview, machines) {
  const names = new Map((machines || []).map(m => [m.id, m.name]));
  const seen = new Set();
  const out = [];
  for (const c of preview?.checkouts || []) {
    if (seen.has(c.machine_id)) continue;
    seen.add(c.machine_id);
    out.push({ id: c.machine_id, name: c.machine_name || names.get(c.machine_id) || c.machine_id });
  }
  if (!out.length) out.push({ id: "", name: "Where it is imported" });
  return out;
}

/// What `import_project` gets as `worktrees` from the picks: one per
/// branch with a machine, in the preview's order.
export function worktreesToAsk(preview, picks) {
  return (preview?.branches || [])
    .filter(b => picks && Object.prototype.hasOwnProperty.call(picks, b.name) && picks[b.name] != null)
    .map(b => ({ branch: b.name, machine_id: picks[b.name] || null }));
}

/// One preview branch's line: its PR or how far ahead, and where it lives.
export function previewLine(b, defaultBranch) {
  const parts = [];
  if (b.name === defaultBranch) parts.push("default branch");
  else if (b.pr != null) parts.push(`PR #${b.pr}${b.pr_title ? ` ${b.pr_title}` : ""}`);
  if (b.ahead > 0 && b.name !== defaultBranch) parts.push(`${b.ahead} ahead`);
  if (!b.in_checkout) parts.push("not in the checkout");
  if (b.worktree) parts.push(b.worktree);
  return parts.join(" · ");
}

/// The open PRs and issues in repos no project has, newest first, grouped
/// by repo: `{ repo, host, items }`. `projects` are the project rows, by
/// `name` (`org/repo`).
export function outsideThreads(view, projects) {
  const known = new Set((projects || []).map(p => String(p.name).toLowerCase()));
  const rows = new Map();
  for (const item of view?.items || []) {
    if (known.has(item.repo.toLowerCase())) continue;
    const key = `${item.host} ${item.repo}`;
    if (!rows.has(key)) rows.set(key, { repo: item.repo, host: item.host, last: 0, items: [] });
    const row = rows.get(key);
    row.items.push(item);
    row.last = Math.max(row.last, item.updated_at);
  }
  for (const row of rows.values()) row.items.sort((a, b) => b.updated_at - a.updated_at);
  return [...rows.values()].sort((a, b) => b.last - a.last);
}

/// "Elsewhere on GitHub", or on Forgejo, or both, from the rows shown.
export function outsideTitle(rows) {
  const github = rows.some(r => r.host === "github.com");
  const other = rows.some(r => r.host !== "github.com");
  return `Elsewhere on ${[github ? "GitHub" : null, other ? "Forgejo" : null].filter(Boolean).join(" and ") || "your git hosts"}`;
}

/// What the import sheet says it found: the checkouts and through which
/// remote, or that there is none yet.
export function checkoutNote(preview) {
  const found = preview?.checkouts || [];
  if (!found.length) return "No checkout on your online machines yet; importing clones it.";
  const one = c => `${c.machine_name} at ${c.folder}${c.remote === "origin" ? "" : ` (through ${c.remote})`}`;
  const more = found.length > 1 ? ` and ${found.length - 1} more` : "";
  return `Found a checkout on ${one(found[0])}${more}.`;
}
