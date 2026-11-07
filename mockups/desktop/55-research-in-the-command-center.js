// ---- Research in the command center ----
// Research has its own place in the sidebar, under Projects: the newest researches (a spinner while the team
// works), the title opens the Research page and + starts one. Research chats no longer show under Chats; Cmd-K
// still finds them. The page lists every research on the account, newest first, in one table; the field on top
// starts a new one. A row opens the research in this window, laid out as a coding agent session.
const CC_RES = [
  { topic: RTOPIC, st: "running", phase: 2, round: 2, sources: 11, at: "24 min ago" },
  { topic: "How do heat pumps cope below -15 °C?", st: "planning", round: 1, sources: 0, at: "1 h ago" },
  { topic: "Best way to learn Catalan as an adult", st: "done", round: 1, sources: 18, at: "1 d ago", took: "14 min" },
  { topic: "Girona to Porto by train: is it worth it?", st: "done", round: 2, sources: 9, at: "4 d ago", took: "22 min" },
  { topic: "Rent control in Barcelona since 2024", st: "stopped", round: 1, sources: 6, at: "8 d ago" },
  { topic: "Ferry fares to Mallorca in winter", st: "failed", round: 1, sources: 0, at: "10 d ago" },
];
const CC_RES_IC = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>';
const CC_RES_PHASES = ["Researching", "Reviewing", "Writing"];
{
  const st = document.createElement("style");
  st.textContent = `.mac .cc-rnew { display: flex; gap: 8px; align-items: center; padding: 6px 6px 6px 12px; border: 1px solid var(--d-border); border-radius: var(--radius); background: color-mix(in srgb, var(--d-surface) 82%, transparent); color: var(--d-dim) }
.mac .cc-rnew:focus-within { border-color: color-mix(in srgb, var(--d-emerald) 60%, transparent) }
.mac .cc-rnew input { flex: 1; min-width: 0; border: 0; outline: 0; background: none; color: var(--d-fg); font: inherit; font-size: 13px; caret-color: var(--d-emerald) }
.mac .cc-rnew input::placeholder { color: var(--d-dim) }
.mac .cc-mt.cc-rt { grid-template-columns: minmax(180px, 2fr) max-content minmax(150px, 1.2fr) 64px 92px }
.mac .cc-mt.cc-rt > .r { cursor: pointer }
.mac .cc-mt.cc-rt .nm b button { padding: 0; border: 0; background: none;font: inherit; color: inherit; cursor: pointer; max-width: 100%; text-align: left; min-width: 0; overflow: hidden; display: flex }
.mac .cc-mt.cc-rt .nm b button:hover { color: var(--d-emerald) }
.mac .cc-rsteps { display: flex; gap: 3px; width: 54px; flex: none }
.mac .cc-rsteps i { flex: 1; height: 3px; border-radius: 2px; background: color-mix(in srgb, var(--d-fg) 8%, transparent) }
.mac .cc-rsteps i.done { background: color-mix(in srgb, #4cb782 70%, transparent) }
.mac .cc-rsteps i.now { background: var(--d-emerald) }`;
  document.head.append(st);
}
const ccResTag = r => r.st === "running" ? '<span class="cc-tag run"><span class="dspin"></span>Running</span>'
  : r.st === "planning" ? '<span class="cc-tag wait">Waiting for your go</span>'
  : r.st === "done" ? '<span class="cc-tag">Done</span>'
  : r.st === "failed" ? '<span class="cc-tag bad">Failed</span>' : '<span class="cc-tag">Stopped</span>';
const ccResProgress = r => r.st === "running"
  ? `<span class="cc-rsteps">${CC_RES_PHASES.map((_, i) => `<i class="${i + 1 < r.phase ? "done" : i + 1 === r.phase ? "now" : ""}"></i>`).join("")}</span><span class="num">${r.round > 1 ? `Round ${r.round} · ` : ""}${CC_RES_PHASES[r.phase - 1]}</span>`
  : r.st === "planning" ? '<span class="num">Plan ready</span>'
  : r.st === "done" ? `<span class="num">${r.round > 1 ? `${r.round} rounds · ` : ""}took ${r.took}</span>`
  : `<span class="num">${r.st === "failed" ? "Failed in round" : "Stopped in round"} ${r.round}</span>`;
// The sidebar's Research group, after Projects.
const ccResGroup = cur => `<div class="cc-grp research"><h3><a class="cc-h3l" href="#" data-dact="dset:rp:list" data-dgo="dWinResearch" title="All research (⌘R)">Research</a><button aria-label="New research" data-dact="dset:rp:new" data-dgo="dWinResearch">${CC_IC.plus}</button></h3>
  ${window.__ccResNone ? '<div class="cc-si none">No research yet</div>' : ""}${(window.__ccResNone ? [] : CC_RES.slice(0, 3)).map(r => `<button class="cc-si"${cur === r.topic ? ' aria-current="true"' : ""} data-dact="dset:rp:open" data-dgo="dWinResearch">${CC_RES_IC}<span class="tt">${r.topic}</span>${r.st === "running" ? '<span class="dspin"></span>' : ""}</button>`).join("")}</div>`;
{
  const base = ccSide;
  ccSide = (cur, o = {}) => {
    const html = base(cur, o);
    if (o.old || o.bare) return html;
    const at = html.search(/<div class="cc-grp"><h3>(Machines|Beep)/);
    return at < 0 ? html : html.slice(0, at) + ccResGroup(cur) + html.slice(at);
  };
}
D.dWinResearch = () => {
  const v = S.d.rp || "list", empty = v === "empty", starting = v === "new", open = v === "open";
  if (open) return rPage();
  const field = `<div class="cc-rnew">${CC_RES_IC}<input type="text" placeholder="What should the team research?" autocomplete="off" spellcheck="false" aria-label="What should the team research?"${starting ? ' value="Rent control in Valencia" disabled' : ""}><button class="dbtn pri small"${starting ? " disabled" : ' data-dact="dset:rp:new" data-dgo="dWinResearch"'}>${starting ? "Starting…" : "Research"}</button></div>`;
  const table = `<div class="cc-mt cc-rt" role="table"><span class="th">Research</span><span class="th">Status</span><span class="th">Progress</span><span class="th">Sources</span><span class="th">Started</span>
    ${CC_RES.map(r => {
      const off = ["stopped", "failed"].includes(r.st) ? " off" : "";
      const go = `data-dact="dset:rp:open" data-dgo="dWinResearch"`;
      return `<span class="nm${off} r" ${go}><b><button type="button" title="${r.topic}"><span>${r.topic}</span></button></b></span><span class="${off.trim()} r" ${go}>${ccResTag(r)}</span><span class="${off.trim()} r" ${go}>${ccResProgress(r)}</span><span class="num${off} r" ${go}>${r.sources || "–"}</span><span class="num${off} r" ${go}>${r.at}</span>`;
    }).join("")}</div>`;
  const body = empty ? `<div class="cc-panel cc-empty"><b>No research yet</b><span>Type a question above to start one.</span></div>`
    : v === "error" ? '<div class="cc-note"><span class="ddot bad"></span><span class="cc-err">Could not reach the server</span></div>'
    : v === "loading" ? `<div class="cc-panel">${[70, 55, 62].map(w => `<div class="cc-skel" style="width:${w}%"></div>`).join("")}</div>`
    : `<div><div class="cc-sec"><h4>Research</h4></div>${table}</div>`;
  window.__ccResNone = empty;
  const side = ccSidePj(open ? CC_RES[0].topic : "");
  window.__ccResNone = false;
  const page = ccWin(side, `<div class="cc-top"><h2>Research</h2><small>${empty ? "Nothing researched yet" : v === "error" || v === "loading" ? "" : "1 running, 1 waiting · 6 researches"}</small></div><div class="cc-view">${field}${body}</div>`);
  return page;
};
VARIANTS.dWinResearch = [
  ["List", () => { S.d.rp = "list"; render(); }, () => (S.d.rp || "list") === "list"],
  ["Starting one", () => { S.d.rp = "new"; render(); }, () => S.d.rp === "new"],
  ...ROPEN.map(([l, v]) => ["Opened: " + l.toLowerCase(), () => { Object.assign(S.d, { rp: "open", rv: v }, rOpenState(v)); render(); }, () => S.d.rp === "open" && S.d.rv === v]),
  ...[["Loading", "loading"], ["Could not load", "error"], ["None yet", "empty"]].map(([l, k]) => [l, () => { S.d.rp = k; render(); }, () => S.d.rp === k]),
];
// Shift+Esc zooms the research's side pane over the lead's chat; the sidebar and head stay.
{
  const orig = D.dWinResearch;
  D.dWinResearch = () => {
    const html = orig();
    return S.d.rzoom && S.d.rp === "open" ? html.replace('<div class="ag-shell', '<div class="ag-shell zoomed') : html;
  };
  const prev = VARIANTS.dWinResearch;
  VARIANTS.dWinResearch = prev.map(([l, f, on]) => [l, () => { S.d.rzoom = false; f(); }, () => !S.d.rzoom && on()]);
  const at = VARIANTS.dWinResearch.findIndex(x => x[0] === "Opened: done");
  VARIANTS.dWinResearch.splice(at + 1, 0, ["Side pane zoomed (Shift+Esc)", () => { Object.assign(S.d, { rp: "open", rv: "done", rzoom: true }, rOpenState("done")); render(); }, () => !!S.d.rzoom]);
}
DESK_SCREENS.find(g => g.group === "Command center").items.push(["dWinResearch", "Research", "CommandCenterWindow, ResearchView"]);
