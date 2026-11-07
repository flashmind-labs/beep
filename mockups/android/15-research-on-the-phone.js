// ---- Research on the phone ----
// A research is a chat with the lead: it opens like any chat, with the topic as its title and a status row
// over the composer. The workspace (paper, notes, sources, team) is its own screen, opened from the bar or
// the status row. Research in the drawer lists every research on the account, from any device.
Object.assign(S, { pr: "plan", pw: "paper", prl: "list", pquote: false });
const PR_ICON = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M9 3h6M10 3v6l-5 9a2 2 0 001.7 3h10.6a2 2 0 001.7-3l-5-9V3"/><path d="M7.5 14h9"/></svg>';
const PR_DOC = '<svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3H7a2 2 0 00-2 2v14a2 2 0 002 2h10a2 2 0 002-2V8z"/><path d="M14 3v5h5M9 13h6M9 17h4"/></svg>';
const pav = a => `<span class="rav" style="background:${a.c};width:22px;height:22px;border-radius:50%;display:inline-grid;place-items:center;font-size:11px;font-weight:700;color:#0A0E0C;flex:none">${a.ini}</span>`;
const pstate = st => st === "run" ? '<span class="spin"></span>' : st === "ok" ? `<span style="color:var(--m-ok);display:grid">${I.check}</span>` : st === "wait" ? '<span class="dot" style="background:var(--m-accent)"></span>' : "";
const PR_SUB = { plan: "Waiting for your go", running: "Researching, 3 agents working", done: "Done in 14 min", quoted: "Done in 14 min", followup: "Round 2, 2 agents working" };
function prBar(sub) {
  return `<div class="bar">
    <button class="ib chunky" style="width:42px;height:42px" data-go="drawer" aria-label="Conversations">${I.menu}</button>
    <div style="flex:1;min-width:0;display:grid;gap:3px">
      <div style="font-weight:600;font-size:16px;line-height:20px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${RTOPIC}</div>
      <div class="sub">${S.pr === "plan" ? '<span class="dot" style="background:var(--m-accent)"></span>' : S.pr === "done" || S.pr === "quoted" ? `<span style="color:var(--m-ok);display:grid">${I.check.replace(/width="16" height="16"/, 'width="12" height="12"')}</span>` : '<span class="spin" style="width:9px;height:9px"></span>'}${sub}</div>
    </div>
    <button class="ib chunky" style="width:42px;height:42px" data-go="workspace" aria-label="Workspace">${PR_DOC}</button>
  </div>`;
}
function prLead(body) {
  return `<div style="display:flex;gap:8px;align-items:center;font-size:12px;color:var(--m-muted)">${pav(ragent("lead"))}Beep, lead</div><div class="reply">${body}</div>`;
}
R.researchChat = () => {
  const st = S.pr;
  const plan = prLead(`Here is the plan. Say go, or tell me what to change.
    <div style="margin-top:10px;display:grid;gap:8px">
      <b style="font-size:13px">Questions</b>
      <ol style="margin:0;padding-left:20px;display:grid;gap:4px"><li>Can manufacturers make the cells at automotive yields?</li><li>Who has announced cars, and for which segment?</li><li>What will a pack cost against lithium-ion?</li></ol>
      <b style="font-size:13px">Team</b>
      <div style="display:grid;gap:6px">${RAGENTS.filter(a => a.id !== "lead" && !a.round).map(a => `<div style="display:flex;gap:8px;align-items:center;font-size:14px">${pav(a)}<span style="flex:1">${a.name}</span><span class="muted" style="font-size:12px">${a.model}</span></div>`).join("")}</div>
      <b style="font-size:13px">Depth</b><span>Standard: about 15 minutes, up to 30 sources.</span>
    </div>`);
  const chips = (list) => `<div style="display:flex;gap:6px 8px;flex-wrap:wrap">${list}</div>`;
  const running = st === "running" || st === "followup";
  // While the team works, a row over the composer says what it is doing; Stop is next to it.
  const status = running ? `<div style="display:flex;align-items:center;gap:10px;padding:8px 10px 8px 14px;margin:0 12px 6px;border-radius:var(--radius);background:var(--m-surface);border:1.5px solid var(--m-ledge)">
      <span class="spin"></span><button class="text-btn" style="flex:1;text-align:left;padding:0;font-size:13px;color:var(--m-ink)" data-go="workspace">${st === "followup" ? "Round 2: 1 scout working, 3 min" : "Researching: 2 of 3 scouts working, 6 min"}</button>
      <button class="text-btn" style="color:var(--m-error);font-size:13px" data-pact="stop">Stop</button></div>` : "";
  const result = prLead(`<b>Probably not in mass-market cars.</b> Expect solid-state cells in a few premium and hybrid models from 2027 to 2028. Yields are still below automotive grade and packs cost about twice lithium-ion, so cars under 40,000 EUR are unlikely before 2030.`);
  let body = `<div class="user">${RTOPIC}</div>${plan}`;
  if (st === "plan") body += chips(`<button class="chunky rchip" style="background:var(--m-user)" data-pact="go">Start</button><button class="chunky rchip" data-toast="Type what to change, like: add a scout on recycling">Change</button>`);
  else body += `<div class="user">Go</div>`;
  if (st === "done" || st === "quoted" || st === "followup") body += result;
  if (st === "done" || st === "quoted") body += chips(`<button class="chunky rchip" data-go="workspace" data-pset="paper">Open the paper</button><button class="chunky rchip" data-toast="Typesetting the PDF">Share PDF</button>`);
  if (st === "followup") body += `<div class="user">What would it take to get one under 30,000 EUR?</div>` + prLead("The paper does not cover that. A scout is looking at cost paths for cheaper cars, and the paper will update.");
  // Quoting from the workspace fills the composer: each quote, then the cursor under it.
  const comp = st === "quoted" ? `<div class="composer"><div class="input" style="align-items:flex-start;padding:12px 14px;min-height:0"><span style="flex:1;white-space:pre-wrap;line-height:20px;font-size:15px"><span class="muted">&gt; Toyota plans limited production in hybrids</span>

Which hybrids, and in what volumes?

<span class="muted">&gt; Mass-market adoption by 2028 is unlikely</span>

<span class="caret" style="display:inline-block;width:2px;height:18px;background:var(--m-brand);vertical-align:-3px"></span></span></div><button class="round coral" aria-label="Send">${IC.up}</button></div>`
    : composer(st === "plan" ? "Say go, or what to change" : running ? "Message the lead" : "Ask a follow-up");
  return prBar(PR_SUB[st]) + `<div class="body">${body}</div>` + status + comp;
};
const PR_LIST = [
  [RTOPIC, "Round 2 · 2 agents working", "run"],
  ["How do heat pumps cope below -15 °C?", "Waiting for your go", "plan"],
  ["Best way to learn Catalan as an adult", "Done · 18 sources · Yesterday", "done"],
  ["Girona to Porto by train: is it worth it?", "Done · 9 sources · 3 Oct", "done"],
  ["Rent control in Barcelona since 2024", "Stopped · 6 sources · 29 Sep", "off"],
];
R.researches = () => {
  const empty = S.prl === "empty";
  const field = `<div class="composer" style="padding:0 16px 4px;flex:none"><div class="input"><span style="flex:1;color:var(--m-dim)">What should the team research?</span></div><button class="round" style="background:var(--m-user)" data-go="researchChat" data-pset="plan" aria-label="Research">${IC.up}</button></div>`;
  if (empty) return back("Research") + field + `<div class="empty">No research yet.<br>A team of agents digs into a question and writes it up as a paper.</div>`;
  const row = ([t, line, k]) => `<button class="card row" style="align-items:center;padding:12px 14px;text-align:left;${k === "off" ? "opacity:.6" : ""}" data-go="researchChat" data-pset="${k === "plan" ? "plan" : k === "run" ? "followup" : "done"}">
    <span style="width:20px;display:grid;place-items:center">${k === "run" ? '<span class="spin"></span>' : k === "plan" ? '<span class="dot" style="background:var(--m-accent)"></span>' : k === "done" ? `<span style="color:var(--m-ok);display:grid">${I.check}</span>` : `<span class="muted" style="display:grid">${I.search}</span>`}</span>
    <span class="grow" style="min-width:0"><b style="font-weight:500;font-size:15px;line-height:20px;display:block">${t}</b><span class="muted" style="font-size:12px">${line}</span></span></button>`;
  return back("Research") + field + `<div class="body" style="gap:8px;padding-top:8px">${PR_LIST.map(row).join("")}</div>`;
};
// The workspace: Paper, Notes, Sources and Team tabs. The paper and notes are selectable, and the selection
// toolbar gets Quote, which goes back to the chat with the quote in the composer.
function prTabs(cur) {
  const t = (id, label) => `<button class="fchip" style="min-height:32px;${cur === id ? "background:var(--m-user);border-color:transparent;font-weight:600" : ""}" data-pset="${id}" data-go="workspace">${label}</button>`;
  return `<div class="chips" style="flex:none;padding:0 16px 8px;display:flex;gap:6px;overflow:auto">${t("paper", "Paper")}${t("notes", "Notes")}${t("sources", "Sources 22")}${t("team", "Team")}</div>`;
}
const prMsg = (id, to, text) => { const a = ragent(id); return `<div style="display:grid;grid-template-columns:22px 1fr;gap:2px 10px"><span>${pav(a)}</span><b style="font-size:12px;font-weight:600;color:var(--m-muted)">${a.name}${to ? ` <span style="color:var(--m-brand)">to ${to}</span>` : ""}</b><span style="grid-column:2;font-size:14px;line-height:20px">${text}</span></div>`; };
const prStep = (label, run) => `<div style="display:flex;gap:8px;align-items:center;font-size:13px;color:var(--m-muted)">${run ? '<span class="spin"></span>' : `<span style="color:var(--m-ok);display:grid">${I.check.replace(/width="16" height="16"/, 'width="13" height="13"')}</span>`}${label}</div>`;
R.workspace = () => {
  const w = S.pw;
  const share = `<button class="ib" aria-label="Share PDF" data-pset="exporting" data-go="workspace">${I.share.replace(/width="16" height="16"/, 'width="20" height="20"')}</button>`;
  const bar = (title, sub, to = "researchChat") => `<div class="bar" style="padding-left:6px"><button class="ib" data-go="${to}" aria-label="Back">${I.back}</button>
    <div style="flex:1;min-width:0;display:grid;gap:2px"><div style="font-weight:600;font-size:16px;line-height:20px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${title}</div><div class="sub">${sub}</div></div>${to === "researchChat" ? share : ""}</div>`;
  if (w === "thread") {
    const a = ragent("a");
    return bar(a.name, `${a.role} · ${a.model}`, "workspace") + `<div class="body" style="gap:12px">
      ${prMsg("lead", "Scout 1", "Find yield numbers from pilot lines, not press claims.")}
      <div style="display:grid;gap:8px;padding-left:32px">${prStep("Searched the web: solid-state pilot line yield")}${prStep("Read QuantumScape Q3 letter")}${prStep("Read Nature Energy, sulfide scale-up")}${prStep("Added 2 sources")}</div>
      ${prMsg("a", "room", "Best public number is 82% of target cycle yield. Treat it as a ceiling.")}
      <div style="display:grid;gap:8px;padding-left:32px">${prStep("Reading a pilot-line yield report", true)}</div>
    </div>`;
  }
  if (w === "room") return bar("Room", "Where the agents talk", "workspace") + `<div class="body" style="gap:14px">
      ${prMsg("lead", "everyone", "Three questions, one each. Post findings here with a source.")}
      ${prMsg("c", "", "Every cost forecast I found keeps packs near 2x lithium-ion to 2030.")}
      ${prMsg("b", "Scout 3", "Does that include Toyota's hybrid packs? They are much smaller.")}
      ${prMsg("c", "Scout 2", "No, those are excluded. Adding a note.")}
      ${prMsg("a", "", "Best public yield is 82% of target. Treat it as a ceiling.")}
    </div>`;
  const tab = w === "quote" || w === "exporting" ? "paper" : w;
  let body;
  if (tab === "paper") {
    const sel = w === "quote";
    const hl = "Mass-market adoption by 2028 is unlikely";
    body = `<div style="display:grid;gap:10px;font-size:15px;line-height:23px">
      <div class="big" style="font-size:26px;line-height:30px">Solid-state batteries in mass-market EVs by 2028</div>
      <span class="muted" style="font-size:12px">Written by Writer, reviewed by Critic. 22 sources.</span>
      <b style="font-size:16px">Summary</b>
      <div style="position:relative${sel ? ";margin-top:46px" : ""}">Solid-state cells will reach a small number of premium and hybrid cars between 2027 and 2028<sup style="color:var(--m-brand);font-weight:600">2, 5</sup>. ${sel ? `<span class="sel">${hl}</span>` : hl}: pilot-line yields are 60 to 82% of target<sup style="color:var(--m-brand);font-weight:600">1</sup> and pack costs stay about twice those of lithium-ion through 2030<sup style="color:var(--m-brand);font-weight:600">3</sup>.
        ${sel ? `<div class="seltb" style="left:60px;top:-46px"><button data-toast="Copied">Copy</button><button data-go="researchChat" data-pset="quoted">Quote</button><button data-toast="Selected all">Select all</button></div>` : ""}</div>
      <b style="font-size:16px">1. Manufacturing</b><div>Sulfide electrolytes need dry rooms that add about 30% to plant capex<sup style="color:var(--m-brand);font-weight:600">4</sup>. No maker reports automotive-grade yields at volume.</div>
      <b style="font-size:16px">2. Who ships first</b><div>Toyota plans limited production in hybrids<sup style="color:var(--m-brand);font-weight:600">2</sup>; Samsung SDI targets the premium segment in 2027<sup style="color:var(--m-brand);font-weight:600">5</sup>.</div>
    </div>`;
  } else if (tab === "notes") {
    body = RNOTES.map(([id, text, src]) => { const a = ragent(id); return `<div style="border-left:3px solid ${a.c};padding:2px 0 2px 12px;display:grid;gap:2px"><b style="font-size:12px;font-weight:600;color:var(--m-muted)">${a.name}</b><span style="font-size:14px;line-height:21px">${text}${src ? ` <sup style="color:var(--m-brand);font-weight:600">${src}</sup>` : ""}</span></div>`; }).join("");
  } else if (tab === "sources") {
    body = RSOURCES.map(([title, by, quote], i) => `<div style="display:grid;grid-template-columns:22px 1fr;gap:3px 8px;padding:10px 0;border-bottom:1px solid var(--m-soft)">
      <span style="color:var(--m-brand);font-weight:600">${i + 1}</span><b style="font-weight:500;font-size:15px;color:var(--m-brand)">${title}</b>
      <span class="muted" style="grid-column:2;font-size:12px">${by}, kept by Scout ${(i % 3) + 1}</span><i style="grid-column:2;font-size:13px;line-height:19px">“${quote}”</i></div>`).join("");
  } else {
    const row = a => { const [doing, s] = RDOING.done[a.id] || ["", ""]; return `<button class="card row" style="align-items:center;padding:10px 12px;text-align:left" data-go="workspace" data-pset="thread">${pav(a)}
      <span class="grow" style="min-width:0"><b style="font-weight:500;font-size:15px">${a.name}</b><span class="muted" style="font-size:12px;display:block;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${doing || a.role}</span><span style="font-size:11px;color:var(--m-dim)">${a.model}</span></span>${pstate(s)}</button>`; };
    body = RAGENTS.filter(a => !a.round).map(row).join("") + `<button class="card row" style="align-items:center;padding:10px 12px;text-align:left" data-go="workspace" data-pset="room">
      <span class="rav" style="width:22px;height:22px;border-radius:50%;display:inline-grid;place-items:center;background:var(--m-soft);font-weight:700;font-size:12px">#</span>
      <span class="grow"><b style="font-weight:500;font-size:15px">Room</b><span class="muted" style="font-size:12px;display:block">14 messages</span></span></button>`;
  }
  const toast = w === "exporting" ? `<div class="toast" style="display:flex;gap:10px;align-items:center"><span class="spin" style="border-top-color:var(--m-page)"></span>Typesetting the PDF</div>` : "";
  return bar(RTOPIC, `<span style="color:var(--m-ok);display:grid">${I.check.replace(/width="16" height="16"/, 'width="12" height="12"')}</span>Done in 14 min`) + prTabs(tab)
    + `<div class="body" style="gap:${tab === "team" ? 8 : 12}px;padding-top:4px">${body}</div>` + toast;
};
// More opens its sheet over the panel; a tile leaves the sheet closed for the way back.
document.addEventListener("click", e => {
  if (e.target.closest("[data-more]")) { S.drawerMode = "more"; render(); }
  else if (S.drawerMode === "more" && e.target.closest(".sheet [data-go], .sheet [data-igo]")) S.drawerMode = "list";
}, true);
document.addEventListener("click", e => {
  const b = e.target.closest("[data-pset], [data-pact]");
  if (!b) return;
  const v = b.dataset.pset, a = b.dataset.pact;
  const to = b.dataset.go || b.dataset.igo;
  if (v && /workspace|iWorkspace/.test(to)) S.pw = v;
  else if (v) S.pr = v;
  if (a === "go") S.pr = "running";
  if (a === "stop") S.pr = "done";
  if (a) render();
}, true);
SCREENS.splice(SCREENS.findIndex(g => g.group === "Group chats"), 0, { group: "Research", items: [
  ["researches", "Research list", "ResearchListScreen"],
  ["researchChat", "Research chat", "ChatScreen, research"], ["workspace", "Workspace", "ResearchWorkspace"],
]});
VARIANTS.researches = [["List", "list"], ["Empty", "empty"]].map(([l, v]) => [l, () => { S.prl = v; render(); }, () => S.prl === v]);
VARIANTS.researchChat = [["Plan", "plan"], ["Running", "running"], ["Done", "done"], ["Quoted from the paper", "quoted"], ["Follow-up", "followup"]].map(([l, v]) => [l, () => { S.pr = v; render(); }, () => S.pr === v]);
VARIANTS.workspace = [["Paper", "paper"], ["Notes", "notes"], ["Sources", "sources"], ["Team", "team"], ["Agent thread", "thread"], ["Room", "room"], ["Quote a selection", "quote"], ["Sharing the PDF", "exporting"]].map(([l, v]) => [l, () => { S.pw = v; render(); }, () => S.pw === v]);
[["iResearches", "researches"], ["iResearchChat", "researchChat"], ["iWorkspace", "workspace"]].forEach(([i, a]) => {
  P[i] = () => iGroup(R[a]()); VARIANTS[i] = VARIANTS[a];
});
