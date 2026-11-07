// ---- Agent instructions: one text for every agent, and per-project text ----
// Layers, first to last: Beep base, role, your instructions, project, this machine (workers), context and task.
// Beep only proposes edits; nothing changes until you accept, and History keeps every version.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .in-tabs { display: flex; gap: 4px; flex-wrap: wrap; margin-bottom: -4px }
.mac .in-tabs .tm-tab .n { font-size: 10.5px; color: var(--d-ink); background: var(--d-coral); border-radius: 999px; padding: 0 5px; font-weight: 600 }
.mac .in-cols { display: grid; grid-template-columns: minmax(0, 1.6fr) minmax(0, 1fr); gap: 14px; align-items: start }
.mac .in-ed { display: grid; gap: 8px; padding: 10px }
.mac .in-ed textarea { width: 100%; height: 300px; box-sizing: border-box; resize: none; border-radius: 8px; border: 1px solid var(--d-border); background: var(--d-ink); color: var(--d-fg); padding: 10px 12px; font-family: var(--mono, ui-monospace, monospace); font-size: 12px; line-height: 18px; scrollbar-width: thin }
.mac .in-ver { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 10px; font-size: 11.5px; color: var(--d-muted) }
.mac .in-ver b { color: var(--d-fg); font-weight: 500 }
.mac .in-ver .sp { margin-left: auto }
.mac .in-acts { display: flex; gap: 6px; flex-wrap: wrap; align-items: center }
.mac .in-acts small { font-size: 11.5px; color: var(--d-muted) }
.mac .in-layers { margin: 0; padding: 6px 0; list-style: none; counter-reset: l }
.mac .in-layers li { counter-increment: l; display: grid; grid-template-columns: 20px minmax(0, 1fr); gap: 0 8px; padding: 6px 10px; font-size: 12.5px; line-height: 17px }
.mac .in-layers li::before { content: counter(l); font-size: 11px; color: var(--d-dim); text-align: right; font-variant-numeric: tabular-nums }
.mac .in-layers li small { grid-column: 2; font-size: 11.5px; color: var(--d-muted) }
.mac .in-layers li.me { background: rgba(94,106,210,.14) }
.mac .in-layers li.me b { color: var(--d-fg) }
.mac .in-layers b { font-weight: 500 }
.mac .in-foot { padding: 8px 12px; border-top: 1px solid var(--d-border); font-size: 11.5px; line-height: 16px; color: var(--d-muted) }
.mac .in-card { display: grid; gap: 8px; padding: 10px 12px }
.mac .in-card + .in-card { border-top: 1px solid var(--d-border) }
.mac .in-card .h { display: flex; align-items: center; gap: 8px; min-width: 0 }
.mac .in-card .h b { font-size: 13px; font-weight: 500 }
.mac .in-card .h small { margin-left: auto; font-size: 11px; color: var(--d-muted); white-space: nowrap }
.mac .in-card .why { font-size: 12.5px; line-height: 18px; color: rgba(227,228,230,.9) }
.mac .in-diff { display: grid; border-radius: 8px; border: 1px solid var(--d-border); background: var(--d-ink); overflow: hidden; font-family: var(--mono, ui-monospace, monospace); font-size: 11.5px; line-height: 18px }
.mac .in-diff span { padding: 0 10px 0 22px; position: relative; white-space: pre-wrap; overflow-wrap: anywhere }
.mac .in-diff span::before { position: absolute; left: 8px; color: var(--d-dim) }
.mac .in-diff .add { background: rgba(94,106,210,.16); color: #9FE3C6 } .mac .in-diff .add::before { content: "+"; color: #4CB782 }
.mac .in-diff .del { background: rgba(235,87,87,.14); color: #F4AFA5 } .mac .in-diff .del::before { content: "-"; color: var(--d-err) }
.mac .in-diff .ctx { color: rgba(227,228,230,.6) }
.mac .in-diff .hd { padding-left: 10px; font-family: inherit; color: var(--d-muted); border-bottom: 1px solid var(--d-border); background: var(--d-elev) }
.mac .in-ev { display: flex; flex-wrap: wrap; gap: 6px; align-items: center }
.mac .in-chip { display: inline-flex; align-items: center; gap: 6px; font-size: 11px; line-height: 18px; padding: 0 8px; border-radius: 999px; border: 1px solid rgba(138,156,240,.35); background: rgba(138,156,240,.07); color: #8A9CF0; text-decoration: none; white-space: nowrap }
.mac .in-chip svg { width: 11px; height: 11px; color: currentColor }
.mac .in-ev > small { font-size: 11px; color: var(--d-dim) }
.mac .in-seg { display: flex; flex-wrap: wrap; gap: 4px }
.mac .in-seg button { padding: 2px 9px; border-radius: 999px; border: 1px solid var(--d-border); background: transparent; color: var(--d-fg); font-size: 12px }
.mac .in-seg button[aria-pressed="true"] { background: rgba(94,106,210,.26); border-color: rgba(94,106,210,.55) }
.mac .in-mach { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 6px 10px; padding: 10px 12px; align-items: center }
.mac .in-mach + .in-mach { border-top: 1px solid var(--d-border) }
.mac .in-mach > b { font-size: 13px; font-weight: 500; display: flex; align-items: center; gap: 8px }
.mac .in-mach > b small { font-weight: 400; font-size: 11.5px; color: var(--d-muted) }
.mac .in-files { grid-column: 1 / -1; display: grid; gap: 3px }
.mac .in-file { display: grid; grid-template-columns: 96px minmax(0, 1fr) auto; gap: 8px; align-items: center; font-size: 12px; min-height: 24px }
.mac .in-file code { font-size: 11.5px; color: rgba(227,228,230,.86); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .in-file .ag { color: var(--d-muted) }
.mac .in-file .r { display: flex; gap: 6px; align-items: center; justify-content: end; font-size: 11.5px; color: var(--d-muted); white-space: nowrap }
.mac .in-file .r .dbtn { min-height: 22px; padding: 0 8px; font-size: 11.5px }
.mac .in-file.miss code, .mac .in-file.miss .ag { color: var(--d-dim) }
.mac .in-ok { color: var(--d-emerald) } .mac .in-warn { color: #F2C94C }
.mac .in-hist { display: grid; grid-template-columns: 44px minmax(0, 1fr) auto; gap: 2px 10px; padding: 8px 12px; align-items: center }
.mac .in-hist + .in-hist { border-top: 1px solid var(--d-border) }
.mac .in-hist .v { font-size: 12px; color: var(--d-muted); font-variant-numeric: tabular-nums }
.mac .in-hist .t { display: grid; line-height: 17px; min-width: 0 }
.mac .in-hist .t b { font-size: 13px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .in-hist .t span { font-size: 11.5px; color: var(--d-muted) }
.mac .in-hist[aria-selected="true"] { background: rgba(94,106,210,.12) }
.mac .in-pj { padding: 10px 12px; display: grid; gap: 8px }
.mac .in-pj ul { margin: 0; padding-left: 16px; display: grid; gap: 2px; font-size: 12.5px; line-height: 18px; color: rgba(227,228,230,.9) }
.mac .in-pj .m { font-size: 11.5px; color: var(--d-muted) }
.mac .cc-ask .in-diff { font-size: 11.5px }
.mac .dbtn[disabled] { opacity: .45; cursor: default }
.mac .rv-pill { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; font-weight: 600; padding: 0 9px; line-height: 21px; border-radius: 999px; border: 1px solid; white-space: nowrap }
.mac .rv-pill.changes { color: #F2C94C; border-color: rgba(242,201,76,.5); background: rgba(242,201,76,.08) }
.mac .rv-pill.ready { color: var(--d-emerald); border-color: rgba(94,106,210,.55); background: rgba(94,106,210,.1) }
.mac .rv-pill.blocked { color: var(--d-err); border-color: rgba(235,87,87,.5); background: rgba(235,87,87,.08) }
.mac .rv-pill.running { color: var(--d-muted); border-color: var(--d-border) }
.mac .rv-pill .dspin { width: 9px; height: 9px }
.mac .rv-facts { display: flex; flex-wrap: wrap; gap: 6px }
.mac .rv-fact { display: inline-flex; align-items: center; gap: 6px; padding: 3px 10px; border-radius: 999px; border: 1px solid var(--d-border); background: var(--d-elev); font-size: 12px; white-space: nowrap }
.mac .rv-fact b { font-weight: 600 }
.mac .rv-fact.bad { border-color: rgba(235,87,87,.5); color: #F4AFA5 }
.mac .rv-fact .ddot.bad { background: var(--d-err) }
.mac .rv-find { display: grid; grid-template-columns: 62px minmax(0, 1fr) auto; gap: 3px 10px; padding: 10px 12px; align-items: start }
.mac .rv-find + .rv-find { border-top: 1px solid var(--d-border) }
.mac .rv-find .n { font-size: 11px; color: var(--d-dim); font-variant-numeric: tabular-nums }
.mac .rv-sev { justify-self: start; font-size: 10.5px; font-weight: 600; line-height: 17px; padding: 0 7px; border-radius: 999px; border: 1px solid var(--d-border); color: var(--d-muted) }
.mac .rv-sev.high { color: var(--d-err); border-color: rgba(235,87,87,.5) }
.mac .rv-sev.medium { color: #F2C94C; border-color: rgba(242,201,76,.45) }
.mac .rv-find .t { display: grid; gap: 2px; min-width: 0 }
.mac .rv-find .t b { font-size: 13px; font-weight: 500; line-height: 18px }
.mac .rv-find .t code { font-size: 11.5px; color: #8A9CF0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .rv-find .t span { font-size: 11.5px; color: var(--d-muted) }
.mac .rv-ver { display: inline-flex; align-items: center; gap: 4px; font-size: 11px; color: var(--d-emerald); white-space: nowrap }
.mac .rv-ver svg { width: 12px; height: 12px }
.mac .rv-ver.wait { color: var(--d-muted) } .mac .rv-ver .dspin { width: 9px; height: 9px }
.mac .rv-shard { display: grid; grid-template-columns: 28px minmax(0, 1fr) auto; gap: 2px 10px; padding: 8px 12px; align-items: center }
.mac .rv-shard + .rv-shard { border-top: 1px solid var(--d-border) }
.mac .rv-shard .t { display: grid; line-height: 17px; min-width: 0 }
.mac .rv-shard .t b { font-size: 12.5px; font-weight: 500; font-family: var(--mono, ui-monospace, monospace) }
.mac .rv-shard .t span { font-size: 11.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .rv-bar { flex: none; display: flex; align-items: center; gap: 8px; flex-wrap: wrap; padding: 10px 16px 14px; border-top: 1px solid var(--d-border) }
.mac .rv-bar small { font-size: 11.5px; color: var(--d-muted) }
.mac .rv-bar .sp { flex: 1 }
.mac .rv-cfl { display: grid; gap: 4px; font-size: 12.5px; line-height: 18px }
.mac .rv-cfl code { font-size: 11.5px }
@container (max-width: 640px) { .mac .in-cols { grid-template-columns: minmax(0, 1fr) } }`;
  document.head.append(st);
})();

const IN_TICK = '<svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>';
const IN_RULES = `# How I work

- Write plainly. No em-dashes, no filler words.
- Keep replies short unless I ask for detail.
- Never push to main and never force push.
- Ask before deploying anything.
- Run the project's checks before saying done.
- In anything public, use placeholders like
  build-box or /srv/project, never real names.

## Rust
- No unwrap or expect in library code.
- cargo fmt, clippy and test, scoped to the
  crates you touched.`;
const inDiff = (lines, head) => `<div class="in-diff">${head ? `<span class="hd">${head}</span>` : ""}${lines.map(([k, t]) => `<span class="${k}">${t}</span>`).join("")}</div>`;
const inChip = (ic, label, toast) => `<a href="#" class="in-chip" data-toast="${toast}">${ic}${label}</a>`;
D.dWinInstructions = () => {
  const side = ccSidePj("");
  const head = `<div class="cc-top"><h2>Instructions</h2><small>Your rules for every agent Beep runs</small><span class="tools"><button class="dbtn quiet small" data-dgo="dWinSettings">Settings</button></span></div>`;
  return ccWin(side, head + SETV.insBody());
};

// Review before merge: reviewers read the branch's diff in shards, a verifier checks each finding, then you act.
window.__rvPane = () => {
  const v = S.d.rv || "changes";
  const run = v === "running", ready = v === "ready", blocked = v === "blocked", stale = v === "stale";
  if (v === "starting") return `<div class="cc-panel cc-empty"><span class="dspin"></span><b>Starting the review of login-copy</b></div>`;
  if (v === "none") return `<div class="cc-panel cc-empty"><b>No review open</b><button class="dbtn" data-dact="dset:psel:branches" data-dgo="dWinProject">Branches</button></div>`;
  const pill = stale ? '<span class="rv-pill changes">Stale</span>' : run ? '<span class="rv-pill running"><span class="dspin"></span>Reviewing</span>'
    : ready ? '<span class="rv-pill ready">Ready</span>' : blocked ? '<span class="rv-pill blocked">Blocked</span>' : '<span class="rv-pill changes">Needs changes</span>';
  const head = `<div class="rv-head"><h3>Review login-copy into main</h3>${pill}</div>`;
  const tools = `<span class="rv-tools"><button class="dbtn" data-dact="dset:ags:diffzoom" data-dgo="dAgentSession">View diff</button><button class="dbtn" data-toast="Opens PR #41 on GitHub">PR #41</button>${run ? "" : '<button class="dbtn" title="Reviews the branch head again" data-toast="Reviews the branch head again">Run again</button>'}</span>`;
  const fact = (html, bad) => `<span class="rv-fact${bad ? " bad" : ""}">${html}</span>`;
  const facts = `<div class="rv-facts">${fact("<b>412</b> changed lines")}${fact("<b>2</b> shards")}${blocked ? fact('<span class="ddot bad"></span>CI failing', true) : run ? fact('<span class="dspin"></span>CI running') : fact('<span class="ddot"></span>CI passing')}${blocked ? fact("Conflicts in 2 files", true) : fact("Merges cleanly")}${fact(`head <code>3f2c1a0</code> · ${run ? "started 2 min ago" : "2 h ago"}`)}${tools}</div>`;
  const shard = (ini, ag, area, sub, state) => `<div class="rv-shard"><span class="cc-ag ${ag}">${ini}</span><span class="t"><b>${area}</b><span>${sub}</span></span>${state}</div>`;
  const doneSt = n => `<span class="pj-st done">${n ? `done · ${n} ${n === 1 ? "finding" : "findings"}` : "done · nothing found"}</span>`;
  const shards = `<div><div class="cc-sec"><h4>Shards</h4></div><div class="cc-panel">
      ${shard("CC", "claude", "apps/web", "268 lines · 6 files · Claude Code on build-box", run ? '<span class="pj-st running"><span class="dspin"></span>reading Login.tsx</span>' : doneSt(ready ? 0 : 2))}
      ${shard("CX", "codex", "crates/core", "144 lines · 3 files · Codex on build-box", run ? '<span class="pj-st running"><span class="dspin"></span>running cargo test</span>' : doneSt(ready ? 0 : 1))}</div></div>`;
  const side = (ver, b) => `<span class="rv-side">${ver === "wait" ? '<span class="rv-ver wait"><span class="dspin"></span>verifying</span>' : `<span class="rv-ver">${IN_TICK}Verified</span>`}${b ? `<button class="dbtn quiet small" data-toast="Dismissed">Dismiss</button>` : ""}</span>`;
  const fnd = (sev, title, loc, sub, ver) => `<div class="rv-find"><span class="rv-sev ${sev}">${sev}</span><span class="t"><b>${title}</b><code>${loc}</code><span>${sub}</span></span>${side(ver, ver !== "wait")}</div>`;
  const f1 = fnd("high", "The reset screen still says which field is wrong", "apps/web/src/reset/copy.ts:18", "Tells someone the email exists, which the PR fixed on login only");
  const f2 = fnd("medium", "Failed logins reset the rate limit counter", "crates/core/src/auth/limit.rs:72", "It should reset after a success, not on every attempt");
  const f3 = fnd("low", "The test checks the old string", "apps/web/src/login/Login.test.tsx:40", "Passes with or without the change");
  let findings;
  if (run) findings = `<div class="cc-panel">${fnd("high", "The reset screen still says which field is wrong", "apps/web/src/reset/copy.ts:18", "From apps/web, so far", "wait")}<div class="cc-empty rv-more">More findings show as the reviewers finish.</div></div>`;
  else if (ready) findings = `<div class="cc-panel cc-empty"><b>No findings</b><span class="rv-sum">2 reviewers read all 412 lines. The verifier dropped 1 finding that was not real: a macro uses the import in limit.rs.</span></div>`;
  else if (blocked) findings = `<div class="cc-panel">${f2}</div>`;
  else findings = `<div class="cc-panel">${f1}${f2}${f3}<div class="rv-find gone"><span class="rv-sev ">dropped</span><span class="t"><b>Unused import in limit.rs</b><span>Not real: a macro uses it</span></span><span class="rv-side"><span></span></span></div></div>`;
  const n = blocked ? "1" : "3";
  const findSec = `<div><div class="cc-sec"><h4>Findings</h4><small>${run ? "reviewing" : ready ? "none open" : `${n} open, ranked`}</small></div>${findings}</div>`;
  const cfl = blocked ? `<div class="cc-note rv-cfl"><span class="ddot bad"></span><span class="rv-cfl-t"><b>Conflicts with main in 2 files</b><span><code>apps/web/src/login/copy.ts</code>, <code>crates/core/Cargo.toml</code></span><b>CI failing</b><span>Checks fail on PR #41. The review still ran.</span></span></div>` : "";
  const staleNote = stale ? `<div class="cc-note"><span class="ddot warn"></span><span><b>login-copy moved to 9e1b2c4 since this review.</b> The findings are for 3f2c1a0; Merge is off until the new head is reviewed.</span><button class="dbtn pri" data-toast="Reviews the branch head again">Review again</button></div>` : "";
  const open = !run && !ready;
  const bar = `<div class="rv-bar">
      <button class="dbtn${open ? " pri" : ""}" ${run || ready ? "disabled" : `title="Starts a task on login-copy with ${n} findings" data-toast="Task started on login-copy"`}>Fix with worker</button>
      <button class="dbtn" ${run ? "disabled" : 'title="The project agent posts it; its card asks you first" data-toast="The project agent posts it; its card asks you first"'}>Post review…</button>
      <button class="dbtn${ready ? " pri" : ""}" ${ready ? 'data-toast="Merge always asks first"' : "disabled"}>Merge…</button>
      <small>${stale ? "Merge is off: the branch moved since this review" : run ? "Actions open when the review is done" : ready ? "Merge always asks first. It never runs on its own." : blocked ? "Merge is off while CI fails and 2 files conflict" : "Merge is off while 3 findings are open. Fix them or dismiss them."}</small></div>`;
  return `${head}${facts}${staleNote}${cfl}<div class="pj-cols">${findSec}${shards}</div>${bar}`;
};
// A review shows in the project page's detail, with Branches lit.
D.dWinReview = () => window.__pjReviewPage();
VARIANTS.dWinReview = [["Needs changes", "changes"], ["Running", "running"], ["Ready", "ready"], ["Blocked", "blocked"], ["Stale", "stale"], ["Starting", "starting"], ["No review open", "none"]]
  .map(([l, k]) => [l, () => { S.d.rv = k; render(); }, () => (S.d.rv || "changes") === k]);

// The project view gets its own instructions, and Review on every PR and branch with work.
{
  const base = D.dWinProject;
  const panel = `<div><div class="cc-sec"><h4>Instructions</h4><small>for this project's agents · v3</small></div><div class="cc-panel in-pj">
      <ul><li>Run the tests with just test.</li><li>The Android app ships from Play; never hand-install APKs.</li><li>Old phones still call the server, so keep replies the same shape.</li></ul>
      <span class="m">Comes after your own instructions. Project chats, reviews and this project's workers read it.</span>
      <div class="in-acts"><button class="dbtn" data-toast="Edit acme/widget's instructions">Edit</button></div></div></div>`;
  D.dWinProject = (...a) => {
    const html = base(...a);
    if ((S.d.pj || "default") === "nocheckout") return html;
    return html.replace("<!--pj-right-->", panel);
  };
}
// Project board: one square tile per kind of thing in the project. Each tile
// shows its count, the few items that matter now and one action; clicking the
// header opens the full list in place of the board (Esc returns). The list
// layout stays one click away in the header.
{
  const st = document.createElement("style");
  st.textContent = `
  .mac .pb-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px }
  /* Three tiles sit three a row, so none is left alone; one column on a narrow window. */
  .mac .pb-grid.n3 { grid-template-columns: repeat(3, minmax(0, 1fr)) }
  @container (max-width: 700px) { .mac .pb-grid.n3 { grid-template-columns: minmax(0, 1fr) } }
  .mac .pb-sec + .pb-sec { margin-top: 20px }
  .mac .pb-sec h4 { margin: 0 0 8px; font-size: 12px; font-weight: 600; color: var(--d-muted) }
  .mac .pb-tile { min-height: 236px; display: flex; flex-direction: column; border-radius: 12px; border: 1px solid var(--d-border); background: var(--d-elev); overflow: hidden }
  .mac .pb-tile.wide { grid-column: span 2; aspect-ratio: auto }
  .mac .pb-tile.hot { border-color: rgba(242,201,76,.55) }
  .mac .pb-tile.full { grid-column: 1 / -1; aspect-ratio: auto }
  .mac .pb-h { display: flex; align-items: center; gap: 8px; padding: 10px 12px 8px; font-size: 12.5px; font-weight: 600; cursor: pointer }
  .mac .pb-h svg { opacity: .8 }
  .mac .pb-h .c { margin-left: auto; font-weight: 500; color: var(--d-muted); font-size: 12px }
  /* How fresh a section's kept data is: its age, or a spinner while it reads again. */
  .mac .sw-age { font-size: 11px; color: var(--d-muted); font-weight: 400; white-space: nowrap }
  .mac .sw-mode { font-size: 11px; font-weight: 500; white-space: nowrap; color: var(--d-muted); border: 1px solid var(--d-border); border-radius: 4px; padding: 0 5px }
  .mac .sw-mode.live { color: var(--d-emerald); border-color: var(--d-emerald) }
  .mac .dspin.sw-spin { width: 9px; height: 9px; border-width: 1.5px; display: inline-block; flex: none }
  .mac .pb-h .c b { color: #F2C94C; font-weight: 600 }
  .mac .pb-big { padding: 0 12px 6px; font-size: 26px; font-weight: 650; letter-spacing: -.02em }
  .mac .pb-big small { font-size: 12px; font-weight: 500; color: var(--d-muted); margin-left: 6px; letter-spacing: 0 }
  .mac .pb-rows { flex: 1; min-height: 0; overflow: hidden; display: flex; flex-direction: column }
  .mac .pb-r { display: flex; align-items: center; gap: 8px; padding: 6px 12px; font-size: 12px; line-height: 16px; min-width: 0; border-top: 1px solid var(--d-border) }
  .mac .pb-r .t { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
  .mac .pb-r .t code, .mac .pb-r code { font-size: 11.5px }
  .mac .pb-r .k { color: var(--d-muted); font-size: 11px; flex: none }
  .mac .pb-chip { flex: none; font-size: 10.5px; padding: 1px 6px; border-radius: 999px; border: 1px solid var(--d-border); color: var(--d-muted) }
  .mac .pb-chip.run { color: #8A9CF0; border-color: rgba(76,183,130,.4) }
  .mac .pb-chip.wait { color: #F2C94C; border-color: rgba(242,201,76,.45) }
  .mac .pb-chip.bad { color: #F08A7E; border-color: rgba(242,166,95,.45) }
  .mac .pb-term { margin: 0 12px; padding: 7px 9px; border-radius: 8px; background: #0f1011; font-family: var(--mono, monospace); font-size: 10.5px; line-height: 15px; color: rgba(227,228,230,.75); white-space: pre; overflow: hidden }
  .mac .pb-f { display: flex; gap: 6px; padding: 8px 12px 10px; border-top: 1px solid var(--d-border) }
  .mac .pb-f .dbtn { font-size: 11.5px; padding: 3px 9px }
  .mac .pb-seg { display: inline-flex; border: 1px solid var(--d-border); border-radius: 8px; overflow: hidden }
  .mac .pb-seg button { font: inherit; font-size: 12px; padding: 4px 10px; background: none; border: 0; color: var(--d-muted); cursor: pointer }
  .mac .pb-seg button[aria-pressed="true"] { background: var(--d-elev); color: inherit }
  .mac .pb-bar { flex: none; width: 64px; height: 2px; border-radius: 1px; background: rgba(255,255,255,.06); overflow: hidden }
  .mac .pb-bar i { display: block; height: 100%; background: rgba(76,183,130,.6); transition: width .4s }
  .mac .pb-sync { padding: 7px 12px; border-top: 1px solid var(--d-border); font-size: 12px; display: grid; gap: 4px }
  .mac .pb-sync .h { display: flex; align-items: center; gap: 8px }
  .mac .pb-sync .h b { font-weight: 600 }
  .mac .pb-sync .h .k { margin-left: auto }
  .mac .pb-sync .m { color: var(--d-muted); font-size: 11px; display: flex; gap: 8px; flex-wrap: wrap }
  .mac .pb-sync .pb-bar { width: 100% }
  .mac .pb-back { display: flex; gap: 6px; font-size: 12px; color: var(--d-muted); margin-bottom: 10px; cursor: pointer }
  `;
  document.head.append(st);

  const I = {
    task: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M9 11l3 3 8-8"/><path d="M20 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2h11"/></svg>',
    pr: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="18" r="2.5"/><path d="M6 8.5v7M18 15.5V9a3 3 0 00-3-3h-4"/><path d="M13 3l-2 3 2 3"/></svg>',
    issue: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="1.5" fill="currentColor"/></svg>',
    branch: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="5" r="2.5"/><circle cx="6" cy="19" r="2.5"/><circle cx="18" cy="7" r="2.5"/><path d="M6 7.5v9M18 9.5c0 5-6 4-11 7.5"/></svg>',
    term: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M13 15h4"/></svg>',
    worker: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="4" y="7" width="16" height="12" rx="3"/><path d="M12 3v4M9 12h.01M15 12h.01"/></svg>',
    check: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 3l8 4v5c0 5-3.5 8-8 9-4.5-1-8-4-8-9V7z"/><path d="M9 12l2 2 4-4"/></svg>',
    mach: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/></svg>',
    sync: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M20 11a8 8 0 00-14.3-4.9L4 8"/><path d="M4 4v4h4"/><path d="M4 13a8 8 0 0014.3 4.9L20 16"/><path d="M20 20v-4h-4"/></svg>',
    doc: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M6 3h9l4 4v14H6z"/><path d="M9 12h7M9 16h5"/></svg>',
  };
  // One machine's sync: state, a progress bar while files move, and what is left.
  const syncRow = (name, state, k, pct, meta) => `<div class="pb-sync"><div class="h"><span class="ddot${state === "offline" ? " off" : ""}"></span><b>${name}</b><span class="k">${chip(k, state)}</span></div>
      ${state === "syncing" ? `<div class="pb-bar"><i style="width:${pct}%"></i></div>` : ""}<div class="m">${meta.map(x => `<span>${x}</span>`).join("")}</div></div>`;
  const chip = (k, t) => `<span class="pb-chip ${k}">${k === "run" ? '<span class="ddot" style="width:6px;height:6px;margin-right:4px;display:inline-block"></span>' : ""}${t}</span>`;
  const r = (t, right = "", left = "") => `<div class="pb-r">${left}<span class="t">${t}</span>${right}</div>`;
  const tile = (key, ic, title, count, big, rows, foot, cls = "") => `<section class="pb-tile ${cls}">
      <div class="pb-h" data-dact="dset:pbo:${key}" data-dgo="dWinProject">${ic}${title}<span class="c">${count}</span></div>
      ${big ? `<div class="pb-big">${big}</div>` : ""}<div class="pb-rows">${rows}</div>${foot ? `<div class="pb-f">${foot}</div>` : ""}</section>`;
  const btn = (t, toast, pri) => `<button class="dbtn${pri ? " pri" : ""}" data-toast="${toast}">${t}</button>`;
  const go = (t, to, pri) => `<button class="dbtn${pri ? " pri" : ""}" data-dgo="${to}">${t}</button>`;

  const tiles = ask => [
    tile("tasks", I.task, "Tasks", ask ? "<b>1 needs you</b>" : "10", `2<small>running</small>`,
      r("Fix the Android build on the new NDK", ask ? chip("wait", "asks") : chip("run", "18 min"))
      + r("Swipeable approvals on iOS", chip("run", "6 min"))
      + r("Login screen copy", chip("wait", "review"))
      + r("Settings screen tests", chip("", "planned")),
      btn("New task", "Describe it to the project agent", true) + btn("Plan", "The project agent proposes tasks")),
    tile("prs", I.pr, `Pull requests<small class="sw-age">updated 4 min ago</small>`, "<b>1 needs reply</b>", `4<small>open</small>`,
      r("<code>#41</code> Login screen copy", chip("wait", "reply"))
      + r("<code>#42</code> Spanish strings", chip("run", "approved"))
      + r("<code>#44</code> Update sqlx to 0.9", '<span class="k">dependabot</span>')
      + r("<code>#43</code> Bump gradle", chip("", "draft")),
      go("Review #41", "dWinReview", true) + btn("Let the agent reply", "The agent drafts a reply; nothing posts without you")),
    tile("issues", I.issue, `Issues<span class="dspin sw-spin" title="Updating"></span>`, "3 untracked", `7<small>open</small>`,
      r("<code>#52</code> Crash on rotate during a call", chip("bad", "bug"))
      + r("<code>#50</code> Dark mode for settings", chip("", "new"))
      + r("<code>#48</code> Onboarding in Spanish", chip("", "4 comments"))
      + r("<code>#31</code> Upload fails above 25 MB", chip("wait", "upstream")),
      btn("Work on one", "Pick an issue, agent and machine", true) + btn("New issue", "Opens a draft issue")),
    tile("branches", I.branch, "Branches", "5 with work", "",
      r("<code>beep/fix-android-build</code>", '<span class="k">+4 · worker</span>')
      + r("<code>beep/ios-approvals</code>", '<span class="k">+2 · worker</span>')
      + r("<code>login-copy</code>", '<span class="k">PR #41</span>')
      + r("<code>spike-widgets</code>", '<span class="k">no PR · 3 d</span>')
      + r("<code>main</code>", '<span class="k">default · in sync</span>'),
      go("Review branch", "dWinReview", true) + btn("New branch", "Creates a worktree")),
    tile("terms", I.term, "Terminals", "2 running", "",
      `<div class="pb-term">$ npm run dev
  ready on :5173
  GET /login 200 12ms</div>`
      + r("<code>dev-server</code> build-box", chip("run", "2 h"))
      + r("<code>logs</code> build-box", chip("run", "1 d"))
      + r("<code>ios</code> mac-box", chip("", "idle")),
      `<button class="dbtn pri" data-dgo="dWinTerminal">New terminal<kbd>⌃\`</kbd></button>` + go("Attach", "dWinTerminal")),
    tile("workers", I.worker, "Workers", "2 of 3 slots", "",
      r("fix-android-build", ask ? chip("wait", "asks") : chip("run", "tests"), '<span class="cc-ag claude" style="width:18px;height:18px;font-size:9px">CC</span>')
      + `<div class="pb-term" style="margin:4px 12px 6px">${ask ? "wants: rm -rf ~/.gradle/caches" : "./gradlew test  62 passed, 3 left"}</div>`
      + r("ios-approvals", chip("run", "build"), '<span class="cc-ag codex" style="width:18px;height:18px;font-size:9px">CX</span>')
      + `<div class="pb-term" style="margin:4px 12px 6px">xcodebuild test  ** TEST SUCCEEDED **</div>`,
      ask ? btn("Allow once", "Allowed once", true) + btn("Deny", "Denied") : go("Open", "dAgentSession", true) + btn("Stop all", "Asks first")),
    tile("checks", I.check, "Checks", "CI on GitHub", "",
      r("<code>main</code>", chip("run", "passing"))
      + r("<code>#41</code> login-copy", chip("run", "passing"))
      + r("<code>#43</code> bump-gradle", chip("bad", "1 failing"))
      + r("<code>beep/fix-android-build</code>", chip("", "not pushed")),
      btn("Why #43 fails", "The project agent reads the failing job", true)),
    tile("actions", I.check, "Actions", "1 running · 1 failed", "",
      r("<code>CI</code> Update sqlx to 0.9", chip("run", "running"))
      + r("<code>CI</code> Bump gradle", chip("bad", "failed"))
      + r("<code>Release</code> Login screen copy", chip("", "passed"))
      + r("<code>CI</code> Login screen copy", chip("", "passed"))
      + r("<code>Nightly</code> Scheduled", chip("", "cancelled")), ""),
    tile("machines", I.mach, "Machines", "2 of 3 online", "",
      r("build-box", '<span class="k">home checkout</span>', '<span class="ddot"></span>')
      + r("mac-box", '<span class="k">Xcode</span>', '<span class="ddot"></span>')
      + r("gpu-box", '<span class="k">offline 2 h</span>', '<span class="ddot off"></span>'),
      btn("Add a checkout", "Pick a machine and folder")),
    tile("sync", I.sync, "File sync", "1 syncing", "",
      syncRow("build-box", "in sync", "", 100, ["12,480 files", "1.2 GB", "checked 1 min ago"])
      + syncRow("mac-box", "syncing", "", 23, ["142 of 610 files", "48 of 210 MB", "2.1 MB/s", "about 1 min"])
      + syncRow("gpu-box", "offline", "", 0, ["3 changes waiting", "seen 2 h ago"]),
      btn("Pause", "Sync paused on every machine") + btn("2 conflicts", "Shows both copies of each file")),
    tile("instr", I.doc, "Instructions", "v3", "",
      r("Run the tests with just test.") + r("Ship Android from Play only.") + r("Keep server replies the same shape."),
      go("Edit", "dWinInstructions")),
  ];

  // Decluttered board: six tiles, no big numbers or footer buttons. A chip only marks what
  // needs you; everything else is muted text. Workers fold into Tasks, checks into PRs,
  // file sync into Machines; instructions move to the ⋯ menu and the chat is the composer.
  const k = t => `<span class="k">${t}</span>`;
  const calmTiles = ask => [
    tile("tasks", I.task, "Tasks", "2 running", "",
      r("Fix the Android build on the new NDK", ask ? chip("wait", "asks") : "")
      + r("Swipeable approvals on iOS")
      + r("Login screen copy", chip("wait", "review"))
      + r("Settings screen tests"), ""),
    tile("prs", I.pr, "Pull requests", "4 open", "",
      r("<code>#41</code> Login screen copy", chip("wait", "reply"))
      + r("<code>#42</code> Spanish strings")
      + r("<code>#44</code> Update sqlx to 0.9", k("dependabot"))
      + r("<code>#43</code> Bump gradle", chip("bad", "checks fail")), ""),
    tile("issues", I.issue, "Issues", "7 open", "",
      r("<code>#52</code> Crash on rotate during a call")
      + r("<code>#50</code> Dark mode for settings")
      + r("<code>#48</code> Onboarding in Spanish"), ""),
    tile("branches", I.branch, "Branches", "5", "",
      r("<code>beep/fix-android-build</code>", k("+4"))
      + r("<code>beep/ios-approvals</code>", k("+2"))
      + r("<code>login-copy</code>", k("PR #41")), ""),
    tile("machines", I.mach, "Machines", "2/3", "",
      r("build-box", k("home"), '<span class="ddot"></span>')
      + r("mac-box", k("syncing 23%"), '<span class="ddot"></span>')
      + r("gpu-box", k("offline 2 h"), '<span class="ddot off"></span>'), ""),
    tile("terms", I.term, "Terminals", "2 running", "",
      r("<code>dev-server</code>", k("2 h"))
      + r("<code>logs</code>", k("1 d")), ""),
    tile("actions", I.check, "Actions", "1 running · 1 failed", "",
      r("<code>CI</code> Update sqlx to 0.9", chip("run", "running"))
      + r("<code>CI</code> Bump gradle", chip("bad", "failed"))
      + r("<code>Release</code> Login screen copy", chip("", "passed"))
      + r("<code>CI</code> Login screen copy", chip("", "passed"))
      + r("<code>Nightly</code> Scheduled", chip("", "cancelled")), ""),
  ];

  // The opened tile: the same rows, all of them, with each row's actions.
  const opened = key => {
    const names = { actions: "Actions", sync: "File sync", tasks: "Tasks", prs: "Pull requests", issues: "Issues", branches: "Branches", terms: "Terminals", workers: "Workers", checks: "Checks", machines: "Machines", instr: "Instructions" };
    // The opened Issues tile: every open issue no task tracks, with Work on it (agent and machine
    // from what the machines offer). Plan and Discuss need a server, so they are hidden without one.
    if (key === "issues") {
      const pick = S.d.pjiss === "pick";
      const row = (n, title, meta, open) => `<div class="pj-task pj-issue${open ? " ask" : ""}"><span class="t"><a href="#" data-toast="Opens issue #${n} on GitHub">#${n}</a> ${title}</span><span class="m">${meta}</span>${open
        ? `<span class="pj-start"><span class="lbl">Agent</span><span class="pb-seg"><button aria-pressed="true">Claude Code</button><button>Codex</button><button>pi</button></span><span class="lbl">Machine</span><span class="pb-seg"><button aria-pressed="true">build-box</button><button>mac-box</button><button>Pick at start</button></span><span class="acts"><button class="dbtn pri" data-toast="Worker started on build-box; #52 is now a task">Start worker</button><button class="dbtn quiet" data-toast="Cancelled">Cancel</button></span></span>`
        : `<span class="acts"><button class="dbtn pri" data-dact="dset:pj:issue" data-dgo="dWinProject">Work on it</button></span>`}</div>`;
      return `<div class="pb-back" data-dact="dset:pbo:" data-dgo="dWinProject">&#8592; Board <span class="dmuted">· esc</span></div>
        <div class="cc-sec"><h4>Issues</h4><small>3 not tracked yet</small></div><div class="cc-panel">
        ${row(52, "App crashes when the phone rotates during a call", "opened by a tester · 2 h ago · bug", pick)}
        ${row(50, "Dark mode for the settings screen", "opened by sam · 3 days ago · enhancement")}
        ${row(48, "Translate onboarding to Spanish", "opened by lee · last week · 4 comments")}</div>`;
    }
    const html = tiles(false).find(t => t.includes(`dset:pbo:${key}"`)) || "";
    return `<div class="pb-back" data-dact="dset:pbo:" data-dgo="dWinProject">&#8592; Board <span class="dmuted">· esc</span></div>`
      + html.replace('class="pb-tile ', 'class="pb-tile full ').replace(/<div class="pb-h"[^>]*>/, '<div class="pb-h">')
      + `<div class="dmuted" style="font-size:12px;margin-top:8px">All ${names[key].toLowerCase()} with filters and each row's actions; the board shows the top few.</div>`;
  };

  const base = D.dWinProject;
  D.dWinProject = (...a) => {
    const v = S.d.pj || "default";
    const list = S.d.pjv === "list";
    const seg = `<span class="pb-seg"><button aria-pressed="${!list}" data-dact="dset:pjv:board" data-dgo="dWinProject">Board</button><button aria-pressed="${list}" data-dact="dset:pjv:list" data-dgo="dWinProject">List</button></span>`;
    if (list || !["default", "ask", "queued"].includes(v)) return base(...a).replace('<span class="tools">', `<span class="tools">${v === "nocheckout" ? "" : seg}`);
    const head = `<div class="cc-top"><span class="cc-pdot" style="background:var(--d-coral);width:10px;height:10px"></span><h2>acme/widget</h2>
      <small>Private · GitHub · main</small>
      <span class="tools">${seg}${(S.d.pjb || "calm") === "calm" ? '<button class="dbtn quiet" style="padding:3px 9px" data-toast="Pin, Review, Instructions, Checks, File sync, Repo settings">⋯</button>' : '<button class="dbtn" data-toast="Pinned acme/widget">Pin</button><button class="dbtn" data-dgo="dWinReview">Review</button>'}</span></div>`;
    const calmB = (S.d.pjb || "calm") === "calm";
    // Tiles in sections, two a row: the work, the code host, the machines.
    const SECS = [["Work", ["tasks", "workers", "branches", "checks"]], ["Code host", ["prs", "issues", "actions"]], ["Machines", ["machines", "terms", "sync"]], ["Project", ["instr"]]];
    const all = (calmB ? calmTiles : tiles)(v === "ask");
    const keyOf = t => (t.match(/dset:pbo:(\w+)/) || [])[1];
    const body = S.d.pbo ? opened(S.d.pbo) : SECS.map(([h, keys]) => {
      const got = keys.map(k => all.find(t => keyOf(t) === k)).filter(Boolean);
      return got.length ? `<div class="pb-sec"><h4>${h}</h4><div class="pb-grid${got.length === 3 ? " n3" : ""}">${got.join("")}</div></div>` : "";
    }).join("");
    return ccWin(ccSidePj("acme/widget"), `${head}<div class="cc-view">${body}</div>${pjBar({ chat: true })}`);
  };
  VARIANTS.dWinProject = [["Board", () => { S.d.pj = "default"; S.d.pjv = "board"; S.d.pbo = ""; S.d.pjb = "calm"; render(); }, () => (S.d.pj || "default") === "default" && S.d.pjv !== "list" && !S.d.pbo && (S.d.pjb || "calm") === "calm"],
    ["Board, detailed", () => { S.d.pj = "default"; S.d.pjv = "board"; S.d.pbo = ""; S.d.pjb = "full"; render(); }, () => S.d.pjb === "full" && S.d.pjv !== "list" && !S.d.pbo],
    ["Board, worker asks", () => { S.d.pj = "ask"; S.d.pjv = "board"; S.d.pbo = ""; render(); }, () => S.d.pj === "ask" && S.d.pjv !== "list"],
    ["Tile opened: File sync", () => { S.d.pj = "default"; S.d.pjv = "board"; S.d.pbo = "sync"; render(); }, () => S.d.pbo === "sync" && S.d.pjv !== "list"],
    ["Tile opened: PRs", () => { S.d.pj = "default"; S.d.pjv = "board"; S.d.pbo = "prs"; render(); }, () => S.d.pbo === "prs" && S.d.pjv !== "list"],
    ["Tile opened: Issues", () => { S.d.pj = "default"; S.d.pjv = "board"; S.d.pbo = "issues"; S.d.pjiss = ""; render(); }, () => S.d.pbo === "issues" && S.d.pjiss !== "pick" && S.d.pjv !== "list"],
    ...VARIANTS.dWinProject.map(([l, f, on]) => [l === "Default" ? "List" : l, () => { S.d.pjv = "list"; S.d.pbo = ""; f(); }, () => S.d.pjv === "list" && on()])];
}
// An imported project before it has a project agent: where it lives, the agents in it now, and the
// sessions your coding agents ran in its folders (Claude Code, Codex, pi, flsh), each resumable.
// Tasks, workers and the project chat come later. Opened by clicking a project anywhere.
{
  const base = D.dWinProject;
  const AG = { claude: "Claude Code", codex: "Codex", pi: "pi", flsh: "flsh" };
  D.dWinProject = (...a) => {
    if (!["imported", "withagent", "pmchat"].includes(S.d.pj)) return base(...a);
    const withAgent = S.d.pj === "withagent";
    const head = `<div class="cc-top"><span class="cc-pdot" style="background:var(--d-coral);width:10px;height:10px"></span><h2>acme/widget</h2>
      <small>github.com · imported 2 days ago</small>
      <span class="tools"><button class="dbtn" data-toast="Opens github.com/acme/widget">GitHub</button></span></div>`;
    // Every machine: a checkout (found by its origin remote, or imported there), none yet with
    // Copy here (Clone here when no online machine can give it), a look in progress, or an offline machine that is looked through when it connects.
    const here = [["MacBook Air", "~/Projects/acme/widget", true, "imported"], ["gpu-box", "/path/to/code/widget", false, "found 2 min ago"]];
    const where = `<div><div class="cc-sec"><h4>Where it lives</h4><small>2 of 5 machines</small></div><div class="cc-panel">
      ${here.map(([m, f, mac, how]) =>
        `<div class="pj-task"><span class="t"><b style="font-weight:500">${m}</b> <code>${f}</code></span><span class="m"><span>${how}</span></span><span class="acts">${mac ? '<button class="dbtn" data-toast="Opens the folder in Finder">Open in Finder</button>' : ""}<button class="dbtn quiet" data-toast="Copied ${f}">Copy path</button><button class="dbtn" data-dgo="dAgentNew">Start an agent here</button></span></div>`).join("")}
      <div class="pj-task"><span class="t" style="color:var(--d-muted)"><b style="font-weight:500;color:var(--d-text)">build-box</b> Not on build-box</span><span class="m"><span>checked 2 min ago</span></span><span class="acts"><button class="dbtn" data-toast="Copies acme/widget from gpu-box to build-box and keeps it syncing">Copy here</button></span></div>
      <div class="pj-task"><span class="t" style="color:var(--d-muted)"><b style="font-weight:500;color:var(--d-text)">mini-box</b> Checking…</span><span class="pj-st running"><span class="dspin"></span>looking for a checkout</span></div>
      <div class="pj-task"><span class="t" style="color:var(--d-muted)"><b style="font-weight:500;color:var(--d-text)">old-box</b> Checks when old-box is online</span><span class="m"><span>offline</span></span></div></div></div>`;
    const agents = `<div><div class="cc-sec"><h4>Agents</h4><small>1 running</small></div><div class="cc-panel">
      <div class="pj-task"><span class="t">Add a retry flag to the uploader</span><span class="pj-st running"><span class="dspin"></span>working</span><span class="m"><span class="ap-ag">Claude Code</span><span>gpu-box</span><span>.worktrees/retry-flag</span><span>2 min ago</span></span><span class="acts"><button class="dbtn" data-dgo="dAgentSession">Open</button></span></div>
      <div class="pj-task"><span class="t" style="color:var(--d-muted)">A coding agent in this folder, on the machine you pick</span><span class="acts"><button class="dbtn pri" data-dgo="dAgentNew">Start an agent here</button></span></div></div></div>`;
    // Custom agents: the named agents the harnesses define for this folder (.claude/agents,
    // .opencode/agent, .codex/agents, .pi/agents, .agents) and the user's own on the machine read.
    // A row opens its prompt read-only; Start fills the bar with "Use the <name> agent to ".
    const CAGS = [["claude", "#E5534B", "reviewer", "project", "Reviews a branch for bugs before it merges", "opus"], ["claude", "#3B82F6", "ios-tester", "project", "Runs the iOS app in the simulator and reports what breaks", ""], ["opencode", "", "docs-writer", "project", "Keeps the README and guides short and current", ""], ["codex", "", "explorer", "user", "Reads code and answers questions without changing it", "gpt-5"], ["flsh", "", "broken", "project", "", ""]];
    const cagRow = ([ag, col, n, sc, d, model]) => d
      ? `<div class="pj-task" style="cursor:pointer" data-dact="dset:cag:${n}" data-dgo="dWinProject"><span class="t"><span class="cc-ag ${ag}" style="width:18px;height:18px;vertical-align:-4px;margin-right:6px">${ag.slice(0, 2).toUpperCase()}</span>${col ? `<span class="ddot" style="background:${col};margin-right:6px"></span>` : ""}<b style="font-weight:500">${n}</b> <span style="color:var(--d-muted)">${d}</span></span><span class="m"><span>${sc === "user" ? "yours" : "project"}</span>${model ? `<span>${model}</span>` : ""}</span><span class="acts" style="grid-column:2;grid-row:1 / span 2"><button class="dbtn small" data-toast="Bar filled: Use the ${n} agent to ">Start</button></span></div>`
      : `<div class="pj-task"><span class="t"><span class="ddot bad" style="margin-right:6px"></span><code>.agents/broken.md</code> <span class="cc-err">could not read its frontmatter: a key is missing its colon</span></span></div>`;
    const custom = `<div><div class="cc-sec"><h4>Custom agents</h4><span class="n">5</span></div><div class="cc-panel">${CAGS.map(cagRow).join("")}</div></div>`;
    const cagSheet = S.d.cag ? `<div style="position:absolute;inset:0;background:rgba(0,0,0,.45);display:flex;align-items:center;justify-content:center;z-index:5" data-dact="dset:cag:" data-dgo="dWinProject"><div class="cc-panel" style="width:min(560px,90%);padding:14px 16px" onclick="event.stopPropagation()"><div class="cc-sec"><h4><span class="cc-ag claude" style="width:18px;height:18px;vertical-align:-4px;margin-right:6px">CC</span>${S.d.cag}</h4><small>Claude Code · project · .claude/agents/${S.d.cag}.md · read-only</small></div>
      <pre style="white-space:pre-wrap;margin:8px 0;font-size:12px;max-height:260px;overflow:auto">You review a branch before it merges. Read the diff against main, run the tests, and list real bugs with the file and line. Skip style nits.</pre>
      <div style="display:flex;gap:6px;justify-content:flex-end"><button class="dbtn quiet" data-dact="dset:cag:" data-dgo="dWinProject">Close</button><button class="dbtn pri" data-toast="Bar filled: Use the ${S.d.cag} agent to ">Start</button></div></div></div>` : "";
    const past = [["claude", "Fix the Android build on CI", "gpu-box", "1 h ago"], ["codex", "Login screen copy", "MacBook Air", "yesterday"], ["flsh", "Plan the widget sync", "gpu-box", "3 days ago"], ["pi", "Tidy the README", "MacBook Air", "last week"]];
    const sessions = `<div><div class="cc-sec"><h4>Past sessions</h4><small style="margin-left:auto;color:var(--d-dim)">Updating…</small></div><div class="cc-panel"><div class="pj-scroll" style="max-height:300px;overflow-y:auto" title="Five rows tall; the rest scrolls">
      ${past.map(([ag, t, m, when]) => `<div class="pj-task"><span class="t">${t}</span><span class="pj-st done">ended</span><span class="m">${pvMark(ag)}<span>${m}</span><span>${when}</span></span></div>`).join("")}</div></div></div>`;
    // The prompt bar is under the page with or without a project agent; sending starts one when
    // there is none and opens the project chat. With an agent, Tasks beside Workers on top.
    // Mirrors project.js and center.js in apps/desktop.
    const stt = k => `<span class="pj-st ${k}">${k === "running" ? '<span class="dspin"></span>' : ""}${k}</span>`;
    const task = (title, state, meta, act) => `<div class="pj-task"><span class="t">${title}</span>${stt(state)}<span class="m">${meta}</span><span class="acts"><button class="dbtn quiet small" data-toast="${act === "Drop" ? "Dropped; its issue closes as not planned" : "Back to planned"}">${act}</button></span></div>`;
    const tasks = `<div><div class="cc-sec"><h4>Tasks</h4><small>1 running · 1 in review · 1 planned · 1 done</small></div><div class="cc-panel">
      ${task("Fix the Android build on the new NDK", "running", `<a href="#" data-toast="Opens issue #44">#44</a><span>Claude Code on build-box</span><span>beep/fix-android-build</span>`, "Drop")}
      ${task("Login screen copy", "review", `<a href="#" data-toast="Opens issue #39">#39</a><a href="#" data-toast="Opens PR #41">PR #41</a><span>beep/login-copy</span>`, "Drop")}
      ${task("Settings screen tests", "planned", `<a href="#" data-toast="Opens issue #46">#46</a><span>after Fix the Android build on the new NDK</span>`, "Drop")}
      ${task("Bump iroh to 0.95", "done", `<a href="#" data-toast="Opens PR #37">PR #37</a>`, "Reopen")}
      ${task("Old onboarding flow", "dropped", `<span>closed as not planned</span>`, "Reopen")}</div></div>`;
    const node = (ini, ag, title, sub, out, o = {}) => `<div class="pj-node${o.child ? " child" : ""}"><span class="cc-ag ${ag}">${ini}</span><span class="t"><b>${title}</b><span>${sub}</span></span>${o.stop ? '<button class="dbtn quiet small" style="grid-column:2;justify-self:start" data-toast="Worker stopped; its task is blocked">Stop</button>' : ""}${out ? `<span class="out">${out}</span>` : ""}</div>`;
    const workers = `<div><div class="cc-sec"><h4>Workers</h4><small>1 running</small></div><div class="cc-panel pj-tree">
      ${node(CC_IC.chat, "beep", "Project agent", "Plans tasks, files issues, places workers", "")}
      ${node("CC", "claude", "Fix the Android build on the new NDK", "Claude Code · build-box · .worktrees/fix-android-build · running · 18 min", "./gradlew assembleDebug  BUILD SUCCESSFUL in 41s", { child: true, stop: true })}
      ${node("CX", "codex", "Bump iroh to 0.95", "Codex · build-box · .worktrees/bump-iroh · ended · 42 min", "Pushed beep/bump-iroh and opened PR #37", { child: true })}</div></div>`;
    const planCard = `<div class="pj-plan"><b>Create 3 tasks?</b><ol><li>Swipeable approvals on iOS <small>mac-box</small></li><li>Settings screen tests <small>machine picked at start, after 1</small></li><li>Release notes for 0.4 <small>machine picked at start, after 2</small></li></ol>
      <div class="acts" style="display:flex;gap:6px"><button class="dbtn pri" data-toast="Approved: the tasks start">Approve</button><button class="dbtn warn" data-toast="Plan dropped">Deny</button></div></div>`;
    if (S.d.pj === "pmchat") {
      const top = `<div class="cc-top"><a href="#" class="dbtn" data-dact="dset:pj:withagent" data-dgo="dWinProject" style="padding:2px 8px">widget</a><h2>Project chat</h2><small>the project agent · plans tasks and runs coding agents</small></div>`;
      const live = ccTurn("Get the iOS app ready for the 0.4 release", `<div class="dsteps">${dStep("Starting the project agent")}${dStep("Reading the open issues and branches")}</div>
        <div>That is three tasks. Each runs as a coding agent session in its own worktree; nothing starts until you approve the plan.</div>${planCard}`);
      return ccWin(ccSidePj("acme/widget"), `${top}<div class="cc-chat"><div class="in">${live}</div></div>${ccComp("Ask the project agent about acme/widget")}`);
    }
    if (withAgent) return ccWin(ccSidePj("acme/widget"), `${head}<div class="cc-view"><div class="pj-cols">${tasks}${workers}</div><div class="pj-cols pj-half">${sessions}<div style="display:grid;gap:14px">${where}${agents}${custom}</div></div></div>${pjBar({ chat: true })}${cagSheet}`);
    return ccWin(ccSidePj("acme/widget"), `${head}<div class="cc-view"><div class="pj-cols pj-half">${sessions}<div style="display:grid;gap:14px">${where}${agents}${custom}</div></div></div>${pjBar()}${cagSheet}`);
  };
  VARIANTS.dWinProject = [...VARIANTS.dWinProject.map(([l, f, on]) => [l, () => { S.d.pj = ["imported", "withagent", "pmchat"].includes(S.d.pj) ? "default" : S.d.pj; f(); }, () => !["imported", "withagent", "pmchat"].includes(S.d.pj) && on()]),
    ["Imported project, no agent yet", () => { S.d.pj = "imported"; render(); }, () => S.d.pj === "imported"],
    ["Imported project with its agent", () => { S.d.pj = "withagent"; render(); }, () => S.d.pj === "withagent"],
    ["Project chat after a prompt", () => { S.d.pj = "pmchat"; render(); }, () => S.d.pj === "pmchat"]];
}
DESK_SCREENS.find(g => g.group === "Command center").items.push(
  ["dWinInstructions", "Instructions", "CommandCenterWindow, InstructionsView"],
  ["dWinReview", "Review before merge", "CommandCenterWindow, BranchReview"]);
