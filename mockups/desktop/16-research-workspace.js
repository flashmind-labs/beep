// ---- Research workspace ----
// A research runs on the server: a lead plans it, scouts research in parallel, a critic challenges them and a
// writer drafts the paper. Each agent can run on its own model. The main chat only gets the plan, a rare
// question and the result; the agents' threads and their shared room live quietly in the panel on the right.
document.head.insertAdjacentHTML("beforeend", `<style>
/* Every column starts with a row of the same height: its name, the tabs, or who a thread belongs to. */
.mac .rhd { flex: none; height: 34px; display: flex; align-items: center; gap: 8px; padding: 0 12px; border-bottom: 1px solid var(--d-border); font-size: 12.5px; min-width: 0 }
.mac .rhd h4 { margin: 0; font-size: 12px; font-weight: 600; color: var(--d-dim) }
.mac .rhd small { font-size: 11.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .rhd b { font-weight: 600 }
.mac .rhd .who { display: grid; line-height: 15px; min-width: 0 }
.mac .rhd .rx { margin-left: auto; border: 0; background: transparent; color: var(--d-muted); display: grid; padding: 4px; border-radius: 6px }
.mac .rhd .rx:hover { color: var(--d-fg) }
.mac .rpane { flex: 1; min-height: 0; overflow: auto; scrollbar-width: none; padding: 12px 14px; display: flex; flex-direction: column; gap: 10px; font-size: 13.5px; line-height: 20px }
.mac .rpane > * { flex: none }
.mac .racts { display: flex; gap: 8px; flex-wrap: wrap }
.mac .rstatus { display: flex; align-items: center; gap: 8px; padding: 6px 10px; border: 1px solid var(--d-border); border-radius: 8px; background: rgba(21,22,23,.82); font-size: 12.5px; color: var(--d-muted) }
.mac .rav { width: 20px; height: 20px; border-radius: 6px; display: grid; place-items: center; font-size: 10px; font-weight: 700; color: var(--c); background: var(--d-elev); border: 1px solid var(--d-border); flex: none }
.mac .racts .dbtn .dspin, .mac .ag-pacts .dbtn .dspin { width: 10px; height: 10px; margin-right: 6px; vertical-align: -1px }
/* A box of rows, as the command center's panels. */
.mac .rbox { border: 1px solid var(--d-border); border-radius: 8px; background: rgba(21,22,23,.82); overflow: hidden; min-width: 0 }
.mac .rbox > * + * { border-top: 1px solid rgba(255,255,255,.05) }
.mac .rempty { padding: 22px 18px; text-align: center; font-size: 13px; color: var(--d-muted) }
.mac .rdoc { width: 100%; max-width: 680px; margin: 0 auto; font-size: 13.5px; line-height: 21px; display: grid; gap: 8px; align-content: start }
.mac .rdoc h1 { margin: 0; font-family: var(--display); font-size: 22px; font-weight: 600; letter-spacing: -0.02em; line-height: 1.15 }
.mac .rdoc h2 { margin: 6px 0 0; font-size: 15px; font-weight: 600 }
.mac .rdoc p { margin: 0 }
.mac .rdoc .by { font-size: 12px; color: var(--d-muted) }
.mac .rpane sup { color: var(--d-emerald); font-size: 10px; font-weight: 600; line-height: 0 }
.mac .rmsg { display: grid; grid-template-columns: 20px minmax(0, 1fr); gap: 2px 8px; font-size: 12.5px; line-height: 18px; padding: 4px 2px }
.mac .rmsg b { font-size: 11.5px; font-weight: 600; color: var(--d-muted); align-self: center }
.mac .rmsg p { grid-column: 2; margin: 0 }
.mac .rmsg .to { color: var(--d-emerald) }
.mac .rbox .rmsg { padding: 8px 12px; font-size: 13px; line-height: 19px }
/* Sources: one table, as the Research page's. */
.mac .rtbl { display: grid; grid-template-columns: 34px minmax(0, 1fr) max-content; font-size: 12px; border: 1px solid var(--d-border); border-radius: 8px; background: rgba(21,22,23,.82); overflow: hidden }
.mac .rtbl > * { padding: 7px 10px; min-width: 0; border-top: 1px solid rgba(255,255,255,.05) }
.mac .rtbl > .th { font-weight: 600; color: var(--d-dim); border-top: 0; padding-top: 7px; padding-bottom: 5px }
.mac .rtbl .n { color: var(--d-muted); font-variant-numeric: tabular-nums }
.mac .rtbl .src { display: grid; gap: 1px; line-height: 17px }
.mac .rtbl .src a { color: var(--d-fg); font-size: 12.5px; font-weight: 500; text-decoration: none }
.mac .rtbl .src a:hover { color: var(--d-emerald) }
.mac .rtbl .src small { color: var(--d-muted); font-size: 11.5px }
.mac .rtbl .src q { color: var(--d-muted); margin-top: 2px }
.mac .rtbl .by { display: flex; align-items: flex-start; gap: 6px; color: var(--d-muted); white-space: nowrap }
.mac .rpane.agents { padding: 10px }
.mac .rpane.thread { gap: 6px; padding: 10px 12px; user-select: text }
.mac .rround { position: relative; margin-left: auto }
.mac .rround .dbtn { color: var(--d-fg) }
.mac .rround .dmenu { position: absolute; left: auto; right: 0; top: 26px; width: 200px; z-index: 5 }
.mac .rag { display: grid; grid-template-columns: 20px minmax(0, 1fr) auto; gap: 0 8px; align-items: center; padding: 7px 8px; border: 0; background: transparent; text-align: left; color: var(--d-fg); width: 100%; font: inherit }
.mac .rag:hover { background: rgba(255,255,255,.04) }
.mac .rag b { font-size: 12.5px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .rag small { grid-column: 2 / -1; font-size: 11.5px; line-height: 16px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .rag .model { grid-column: 2 / -1; font-size: 10.5px; line-height: 14px; color: var(--d-dim) }
.mac .ag-pane.rp { position: relative }
.mac .dsaved { position: absolute; left: 50%; bottom: 46px; transform: translateX(-50%); z-index: 5; display: flex; gap: 12px; align-items: center; max-width: calc(100% - 32px); padding: 5px 6px 5px 14px; border-radius: var(--radius); background: var(--d-fg); color: var(--d-ink); font-size: 13px; white-space: nowrap; box-shadow: 0 6px 18px rgba(0,0,0,.45) }
.mac .rp .dsaved { bottom: 56px }
.mac .rp .dsaved .dspin { border-color: color-mix(in srgb, var(--d-ink) 30%, transparent); border-top-color: var(--d-ink) }
.mac .dsaved > span { overflow: hidden; text-overflow: ellipsis }
.mac .dsaved .dbtn { min-height: 24px; background: var(--d-ink); border-color: transparent; color: var(--d-fg) }
.mac .dsaved.bad { background: var(--d-err) }
.mac .dsaved:not(:has(button)) { padding-right: 14px }
.mac .dinput input { flex: 1; min-width: 0; border: 0; outline: 0; background: none; color: var(--d-fg); font: inherit; padding: 0 }
.mac .dinput input::placeholder { color: var(--d-dim) }
.mac .drt .cc-tag { flex: none }
/* An opened research: the head names it and says where it is; the pane's tabs hold the paper, notes, sources and the team. */
.mac .dhead .rdocic { display: grid; color: var(--d-dim); flex: none }
.mac .dhead .tt small { white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .dhead .cc-rsteps { flex: none }
.mac .ag-tab.rt em { opacity: .7 }
.mac .rpane.draft { padding-top: 16px }
</style>`);
Object.assign(S.d, { rs: "plan", rtab: "agents", ropen: [] });
const RTOPIC = "Will solid-state batteries be in mass-market EVs by 2028?";
const RAGENTS = [
  { id: "lead", ini: "L", name: "Lead", role: "Plans and coordinates", model: "Claude Sonnet 5.5", c: "var(--d-coral)" },
  { id: "a", ini: "1", name: "Scout 1", role: "Manufacturing and yields", model: "DeepSeek V4.1 Flash", c: "#4CB782" },
  { id: "b", ini: "2", name: "Scout 2", role: "Who ships what, and when", model: "GLM 5.3 Flash", c: "#8A9CF0" },
  { id: "c", ini: "3", name: "Scout 3", role: "Cost per kWh", model: "DeepSeek V4.1 Flash", c: "#F2C94C" },
  // A follow-up round's scout: only listed once the user asked the follow-up.
  { id: "d", ini: "4", name: "Scout 4", role: "What gets a pack under 30,000 EUR cars", model: "DeepSeek V4.1 Flash", c: "#6BA6B9", round: 2 },
  { id: "critic", ini: "R", name: "Critic", role: "Challenges weak claims", model: "GPT-6 Luna", c: "#939BDA" },
  { id: "writer", ini: "W", name: "Writer", role: "Drafts the paper", model: "Claude Haiku 5.5", c: "var(--d-fg)" },
];
// A tile with the agent's letter in its color, as the command center's agent tiles.
const rav = a => `<span class="rav" style="--c:${a.c}">${a.ini}</span>`;
const ragent = id => RAGENTS.find(a => a.id === id);
// What each agent is doing, by research state. A blank line means it has not started.
const RDOING = {
  plan: { lead: ["Waiting for your go", "wait"] },
  running: { lead: ["Watching the room", "run"], a: ["Reading a pilot-line yield report", "run"], b: ["Comparing announced start dates", "run"], c: ["Searched the web: cell cost forecast", "run"], critic: ["Starts after the scouts", ""], writer: ["Starts after review", ""] },
  // Round 2: the round 1 scouts are done, the new one works, the critic and writer come after.
  followup: { lead: ["Watching the room", "run"], a: ["Done: 9 notes, 5 sources", "ok"], b: ["Done: 7 notes, 6 sources", "ok"], c: ["Done: 6 notes, 4 sources", "ok"], d: ["Reading a cell cost curve", "run"], critic: ["Starts after the scouts", ""], writer: ["Starts after review", ""] },
  reviewing: { lead: ["Coordinating", "run"], a: ["Added 2 sources", "run"], b: ["Read Toyota battery roadmap", "run"], c: ["Added a note", "run"], critic: ["Checking the yield claims", "run"], writer: ["Starts after review", ""] },
  writing: { lead: ["Watching the room", "run"], a: ["Done: 9 notes, 5 sources", "ok"], b: ["Done: 7 notes, 6 sources", "ok"], c: ["Done: 6 notes, 4 sources", "ok"], critic: ["Checking the draft", "run"], writer: ["Writing the paper", "run"] },
  done: { lead: ["Posted the result", "ok"], a: ["Done: 9 notes, 5 sources", "ok"], b: ["Done: 7 notes, 6 sources", "ok"], c: ["Done: 6 notes, 4 sources", "ok"], critic: ["Done: 4 challenges", "ok"], writer: ["Done: the paper", "ok"] },
  stopped: { a: ["9 notes, 5 sources", ""], b: ["7 notes, 6 sources", ""], c: ["6 notes, 4 sources", ""], critic: ["4 challenges", ""] },
};
const rdoing = id => (RDOING[S.d.rs === "sources" ? "done" : ["failed", "interrupted"].includes(S.d.rs) ? "stopped" : S.d.rs] || {})[id] || ["", ""];
const rstate = st => st === "run" ? '<span class="dspin"></span>' : st === "ok" ? `<span class="dok">${DI.check}</span>` : st === "wait" ? '<span class="ddot" style="background:var(--d-coral)"></span>' : "<span></span>";
const RSOURCES = [
  ["Pilot line update, Q3 2026", "QuantumScape investor letter", "Cycle yield on the Cobra line reached 82% of target."],
  ["Toyota battery roadmap briefing", "Toyota press release, 2026", "Limited production in 2027 to 2028, first in hybrids."],
  ["Solid-state cell cost outlook", "BloombergNEF, 2026", "Pack cost stays 1.8x to 2.4x lithium-ion through 2030."],
  ["Sulfide electrolyte scale-up", "Nature Energy, 2026", "Dry-room requirements add about 30% to plant capex."],
  ["Samsung SDI 900 Wh/L cell", "Samsung SDI, 2026", "Mass production targeted for 2027, premium segment."],
];
const RNOTES = [
  ["a", "Yields on pilot lines sit between 60% and 82% of target. Nobody reports automotive-grade yields yet.", "1, 4"],
  ["b", "Toyota, Samsung SDI and QuantumScape name 2027 to 2028 for first cars, all in premium or hybrid models.", "2, 5"],
  ["c", "Pack cost stays about twice lithium-ion through 2030 in every forecast found.", "3"],
  ["b", "No maker has announced a sub-40,000 EUR model with solid-state cells.", ""],
];
// An opened research is a page of the command center, laid out as a coding agent session: the lead's chat on the
// left in the session's log and field, and the paper, notes, sources and the team as tabs of the side pane.
const rDone = () => ["done", "sources", "followup"].includes(S.d.rs);
const rBusy = () => ["running", "followup", "reviewing", "writing"].includes(S.d.rs);
function rConv() {
  const st = S.d.rs;
  const you = text => `<div class="dcoderen user"><b>You</b><span>${text}</span></div>`;
  const says = html => `<div class="dcoderen agent md">${html}</div>`;
  const plan = says(`<p>Three questions, one scout each:</p><ol><li>Can manufacturers make the cells at automotive yields?</li><li>Who has announced cars, and for which segment?</li><li>What will a pack cost against lithium-ion?</li></ol><p>Standard depth: about 15 minutes, up to 30 sources. Say go, or what to change.</p>`);
  const start = st === "plan" ? `<div class="racts"><button class="dbtn pri" data-ract="go">Start<kbd>&#8984;${DI.ret}</kbd></button></div>` : "";
  const line = { running: "Researching: 3 of 3 scouts working, 6 min", followup: "Round 2: 1 of 1 scout working, 3 min",
    reviewing: "The critic and the lead are reviewing, 9 min", writing: "The writer is drafting, 12 min" }[st];
  const status = line ? `<div class="rstatus"><span class="dspin"></span>${line}</div>` : "";
  // After a result the lead stays: it answers from the paper when it can, and sends the team on
  // another round when the question needs research. That round revises the same paper.
  const follow = st === "followup" ? you("What would it take to get one under 30,000 EUR?")
    + says("<p>The paper does not cover that. A scout is looking at cost paths for cheaper cars, and the paper will update.</p>") : "";
  const result = rDone() ? says("<p><b>Probably not in mass-market cars.</b> Expect solid-state cells in a few premium and hybrid models from 2027 to 2028. Yields are still below automotive grade and packs cost about twice lithium-ion, so cars under 40,000 EUR are unlikely before 2030.</p>")
    + (st === "followup" ? "" : `<div class="racts"><button class="dbtn" data-ract="tab:draft">Open the paper</button>${S.d.rexport === "busy" ? '<button class="dbtn" disabled><span class="dspin"></span>Exporting</button>' : '<button class="dbtn" data-dact="dset:rexport:saved">Export PDF</button>'}</div>`) : "";
  const quote = S.d.rquote ? `<span class="txt" style="white-space:pre-wrap"><span style="color:var(--d-muted)">&gt; Toyota plans limited production in hybrids</span>

Which hybrids, and in what volumes?

<span style="color:var(--d-muted)">&gt; Mass-market adoption by 2028 is unlikely: pilot-line yields are 60 to 82% of target</span>

<span class="caret"></span></span>`
    : `<span class="txt ph">${st === "plan" ? "Say go, or what to change" : rDone() ? "Ask a follow-up" : "Message the lead"}</span>`;
  return `<div class="ag-conv"><div class="dbody" id="agbody"><div class="dagmain"><div class="dcoderlog">${you(RTOPIC)}${plan}${start}${st !== "plan" ? you("Go") : ""}${result}${follow}${status}</div></div></div>
    <div class="dinput small" id="agfield">${quote}</div></div>`;
}
// The workspace tabs' bodies, the team's list, an agent's thread and the room.
function rBody(tab) {
  const st = S.d.rs, done = rDone();
  const empty = text => `<div class="rbox rempty">${text}</div>`;
  if (tab === "draft") {
    if (st === "plan") return `<div class="rpane">${empty("The workspace fills in once the team starts.")}</div>`;
    if (!done) return `<div class="rpane">${empty(["failed", "stopped", "interrupted"].includes(st) ? "No paper was written." : "The writer starts after the critic's review.")}</div>`;
    return `<div class="rpane draft"><div class="rdoc"><span class="by">Written by Writer, reviewed by Critic. 22 sources.</span><h1>Solid-state batteries in mass-market EVs by 2028</h1>
    <h2>Summary</h2><p>Solid-state cells will reach a small number of premium and hybrid cars between 2027 and 2028<sup>2, 5</sup>. ${S.d.rquote ? `<span style="position:relative"><span style="position:absolute;left:30%;top:-34px;z-index:2"><button class="dbtn pri" style="min-height:24px;box-shadow:0 4px 14px rgba(0,0,0,.4)" data-toast="Quoted in the message">Quote<kbd>&#8984;'</kbd></button></span><mark style="background:rgba(80,120,190,.55);color:inherit">Mass-market adoption by 2028 is unlikely: pilot-line yields are 60 to 82% of target</mark></span>` : "Mass-market adoption by 2028 is unlikely: pilot-line yields are 60 to 82% of target"}<sup>1</sup> and pack costs stay about twice those of lithium-ion through 2030<sup>3</sup>.</p>
    <h2>1. Manufacturing</h2><p>Sulfide electrolytes need dry rooms that add about 30% to plant capex<sup>4</sup>. No maker reports automotive-grade yields at volume.</p>
    <h2>2. Who ships first</h2><p>Toyota plans limited production in hybrids<sup>2</sup>; Samsung SDI targets the premium segment in 2027<sup>5</sup>.</p></div></div>
    <div class="ag-pacts"><span class="meta">Markdown, 22 sources</span><span class="sp"></span><button class="dbtn" data-dact="dset:rexport:copied">Copy</button>${S.d.rexport === "busy" ? '<button class="dbtn" disabled><span class="dspin"></span>Exporting</button>' : '<button class="dbtn" data-dact="dset:rexport:saved">Export PDF</button>'}</div>`;
  }
  if (tab === "notes") return `<div class="rpane">${st === "plan" ? empty("No notes yet.") : `<div class="rbox">${RNOTES.map(([id, text, src]) => { const a = ragent(id); return `<div class="rmsg">${rav(a)}<b>${a.name}</b><p>${text}${src ? ` <sup>${src}</sup>` : ""}</p></div>`; }).join("")}</div>`}</div>`;
  if (tab === "sources") {
    if (st === "plan") return `<div class="rpane">${empty("No sources yet.")}</div>`;
    const kept = [["quantumscape.com", "a"], ["global.toyota", "b"], ["about.bnef.com", "c"], ["nature.com", "a"], ["samsungsdi.com", "b"]];
    return `<div class="rpane"><div class="rtbl" role="table"><span class="th">#</span><span class="th">Source</span><span class="th">Kept by</span>${RSOURCES.map(([title, , quote], i) => {
      const a = ragent(kept[i][1]);
      return `<span class="n">${i + 1}</span><span class="src"><a href="#" data-toast="Opens in the browser">${title}</a><small>${kept[i][0]}</small><q>${quote}</q></span><span class="by">${rav(a)}${a.name}</span>`;
    }).join("")}</div></div>`;
  }
  if (tab === "room") {
    const m = (id, to, text) => { const a = ragent(id); return `<div class="rmsg">${rav(a)}<b>${a.name}${to ? ` <span class="to">to ${to}</span>` : ""}</b><p>${text}</p></div>`; };
    return `<div class="rhd"><b>Room</b><small>14 messages</small></div><div class="rpane thread">
        ${m("lead", "", "Three questions, one each. Post findings here with a source.")}
        ${m("c", "", "Every cost forecast I found keeps packs near 2x lithium-ion to 2030.")}
        ${m("b", "Scout 3", "Does that include Toyota's hybrid packs? They are much smaller.")}
        ${m("c", "Scout 2", "No, those are excluded. Adding a note.")}
        ${m("a", "", "Best public yield is 82% of target. Treat it as a ceiling.")}
      </div>`;
  }
  if (tab !== "agents") {
    const a = ragent(tab);
    return `<div class="rhd">${rav(a)}<span class="who"><b>${a.name}</b><small>${a.role} &middot; ${a.model}</small></span></div><div class="rpane thread">
        <div class="rmsg">${rav(ragent("lead"))}<b>Lead <span class="to">to Scout 1</span></b><p>Find yield numbers from pilot lines, not press claims.</p></div>
        <div class="dsteps">${dStep("Searched the web: solid-state pilot line yield")}${dStep("Read QuantumScape Q3 letter")}${dStep("Read Nature Energy, sulfide scale-up")}${dStep("Added 2 sources")}${dStep("Reading a pilot-line yield report", "run")}</div>
        <div class="rmsg">${rav(a)}<b>Scout 1</b><p>Best public number is 82% of target cycle yield. Treat it as a ceiling.</p></div>
      </div>`;
  }
  // With more than one round, a picker shows one round's scouts at a time, newest first;
  // the lead, critic and writer work in every round and always show.
  const row = a => { const [doing, s] = rdoing(a.id); return `<button class="rag" data-ract="agent:${a.id}">${rav(a)}<b>${a.name}</b>${rstate(s)}<small>${doing || a.role}</small><span class="model">${a.model}</span></button>`; };
  const rounds = st === "followup" ? 2 : 1, pick = Math.min(S.d.rround ?? rounds, rounds);
  const scout = a => a.id !== "lead" && a.id !== "critic" && a.id !== "writer";
  const rows = [ragent("lead"), ...RAGENTS.filter(a => scout(a) && (a.round ?? 1) === pick), ragent("critic"), ragent("writer")].map(row).join("");
  const picker = rounds > 1 ? `<div class="rhd"><h4>Agents</h4><span class="rround"><button class="dbtn small" data-ract="rounds" aria-haspopup="menu" aria-expanded="${!!S.d.rmenu}">Round ${pick}<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg></button>${S.d.rmenu ? `<span class="dmenu" role="menu">${Array.from({ length: rounds }, (_, i) => rounds - i).map(n => `<button class="mi" role="menuitemradio" aria-checked="${n === pick}" data-ract="round:${n}"><b>Round ${n}</b>${n === pick ? `<span class="ok">${DI.check}</span>` : ""}<small>${n === 1 ? "3 scouts" : "1 scout"}</small></button>`).join("")}</span>` : ""}</span></div>` : "";
  return `${picker}<div class="rpane agents">
    <div class="rbox">${rows}</div>
    <div class="rbox"><button class="rag" data-ract="room"><span class="rav" style="--c:var(--d-muted)">#</span><b>Room</b><span></span><small>${st === "plan" ? "Opens when the team starts" : "14 messages"}</small></button></div>
  </div>`;
}
function rPane() {
  const st = S.d.rs, done = rDone(), tab = S.d.rtab;
  const t = (id, label, n, close) => `<button class="ag-tab${close ? " rt" : ""}${tab === id ? " on" : ""}" role="tab" aria-selected="${tab === id}" data-ract="tab:${id}" title="${label}"><span>${label}</span>${n ? `<small>${n}</small>` : ""}${close ? `<em data-ract="close:${id}" aria-label="Close ${label}" title="Close (&#8984;W)">&#215;</em>` : ""}</button>`;
  const open = S.d.ropen.map(id => id === "room" ? t("room", "Room", 0, true) : t(id, ragent(id).name, 0, true)).join("");
  const tabs = t("draft", "Draft") + t("notes", "Notes", st === "plan" ? 0 : done ? 22 : RNOTES.length) + t("sources", "Sources", st === "plan" ? 0 : done ? 22 : 11) + t("agents", "Agents") + open;
  const saved = S.d.rexport === "busy" ? '<div class="dsaved" role="status"><span class="dspin"></span><span>Typesetting the PDF</span></div>'
    : S.d.rexport === "saved" ? '<div class="dsaved" role="status"><span>Saved Solid-state EVs 2028.pdf to Downloads</span><button class="dbtn" data-toast="Shows the PDF in Finder">Show in Finder</button></div>'
    : S.d.rexport === "copied" ? '<div class="dsaved" role="status"><span>Copied the paper as Markdown</span></div>' : "";
  return `<div class="ag-div" title="Drag to resize, double-click to reset" data-toast="Drag to resize, double-click to reset"></div><aside class="ag-pane rp"><div class="ag-phead" role="tablist"><div class="ag-tabs">${tabs}</div></div>${rBody(tab)}${saved}</aside>`;
}
// The head: the question, where it is (with the running round's steps), the facts, and Stop while it works.
function rHead() {
  const st = S.d.rs;
  const chip = (cls, label, spin) => `<span class="cc-tag${cls ? ` ${cls}` : ""}">${spin ? '<span class="drun"></span>' : ""}${label}</span>`;
  const steps = now => `<span class="cc-rsteps">${[1, 2, 3].map(i => `<i class="${i < now ? "done" : i === now ? "now" : ""}"></i>`).join("")}</span>`;
  let tag = chip("run", "Researching", true) + steps(1), facts = "11 sources";
  if (st === "plan") [tag, facts] = [chip("wait", "Waiting for your go"), ""];
  else if (st === "done" || st === "sources") [tag, facts] = [chip("", "Done"), "took 14 min &middot; 22 sources"];
  else if (st === "followup") [tag, facts] = [chip("run", "Researching", true) + steps(1), "Round 2 &middot; 22 sources"];
  else if (st === "reviewing") [tag, facts] = [chip("run", "Reviewing", true) + steps(2), "11 sources"];
  else if (st === "writing") [tag, facts] = [chip("run", "Writing", true) + steps(3), "15 sources"];
  else if (st === "failed") [tag, facts] = [chip("bad", "Failed"), "15 sources"];
  else if (st === "stopped") [tag, facts] = [chip("", "Stopped"), "15 sources"];
  else if (st === "interrupted") [tag, facts] = [chip("", "Interrupted"), "Server restarted &middot; 15 sources"];
  return `<div class="dhead"><span class="rdocic">${R_DOC}</span><span class="tt"><b>${RTOPIC}</b>${facts ? `<small>${facts}</small>` : ""}</span>${tag}${rBusy() ? `<button class="dbtn warn small" data-ract="stop">Stop</button>` : ""}</div>`;
}
// The research page: the sidebar stays, the head and the shell are the session's.
const rPage = () => ccWin(ccSidePj(CC_RES[0].topic), `<div class="cc-top"><h2>Research</h2></div>${rHead()}<div class="ag-shell">${rConv()}${rPane()}</div>`);
// Starting one: Cmd-R (or Cmd-Shift-Return) sends a typed prompt straight to a research. Nothing offers one;
// a research starts only when the user asks for it. The command center opens it on the plan.
D.dResearchStart = () => deskScene(dBar(dPrompt(`${RTOPIC}<span class="caret"></span>`)
  + dHints([[DI.ret, "ask"], ["&#8984;R", "research"], ["esc", "to hide"]])));
// The bar's Research view (Cmd-R, also in the tray): every research on the account, newest first, from the
// server, so it lists the ones started on another computer too. Return opens one in the command center. The field on
// top starts a new one; Cmd-R in a chat with text typed researches it straight away.
const RLIST = [
  [RTOPIC, "Round 2 · Reviewing · 11 sources · 09:12", "run"],
  ["How do heat pumps cope below -15 °C?", "Plan ready · 08:40", "plan"],
  ["Best way to learn Catalan as an adult", "took 14 min · 18 sources · Yesterday", "done"],
  ["Girona to Porto by train: is it worth it?", "2 rounds · took 22 min · 9 sources · 3 Oct", "done"],
  ["Rent control in Barcelona since 2024", "Stopped in round 1 · 6 sources · 29 Sep", "stopped"],
  ["Ferry fares to Mallorca in winter", "Failed in round 1 · 27 Sep", "failed"],
];
const R_DOC = '<svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>';
const R_CHIP = {
  run: '<span class="cc-tag run"><span class="drun"></span>Running</span>', plan: '<span class="cc-tag wait">Waiting for your go</span>',
  done: '<span class="cc-tag">Done</span>', stopped: '<span class="cc-tag">Stopped</span>', failed: '<span class="cc-tag bad">Failed</span>',
};
const rField = v => `<div class="dinput"><input placeholder="What should the team research?" autocomplete="off" spellcheck="false" aria-label="What should the team research?"${v ? ` value="${v}"` : ""}><kbd>&#8984;R</kbd></div>`;
Object.assign(S.d, { rl: "list" });
D.dResearches = () => {
  const st = S.d.rl;
  const head = `<div class="dhead"><span class="tt"><b>Research</b><small>${st === "empty" ? "Nothing researched yet" : "1 running, 1 waiting · 6 researches"}</small></span></div>`;
  const field = rField(st === "new" ? "Rent control in Valencia" : "");
  const errLine = st === "error" ? '<div class="derr" style="padding:8px 18px 0">Could not reach the server</div>' : "";
  if (st === "empty") return deskScene(dBar(field + head + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--d-dim);display:grid">${R_DOC}</span><b style="font-size:14px">No research yet.</b><span class="dmuted" style="display:block">A team of agents digs into a question and writes it up as a paper.</span></div>` + dHints([[DI.ret, "research"], ["esc", "back"]])));
  const sel = st === "new" ? -1 : st === "actions" ? 2 : 0;
  const rows = RLIST.map(([topic, line, k], i) => {
    const cur = i === sel;
    const off = k === "stopped" || k === "failed";
    // Export replaces the chip while the row's actions show.
    const tail = cur && st === "actions"
      ? `<span class="acts"><button class="dbtn" data-toast="Saved Paper.pdf to Downloads">Export PDF</button></span>`
      : R_CHIP[k];
    return `<div class="drt${off ? " off" : ""}" role="option" aria-selected="${cur}" data-dact="dset:rp:open" data-dgo="dWinResearch"><span class="ic">${R_DOC}</span><span class="tt"><b>${topic}</b><small>${line}</small></span>${tail}</div>`;
  }).join("");
  const hints = st === "new" ? [[DI.ret, "research"], ["esc", "back"]]
    : st === "actions" ? [["tab", "hide actions"], ["esc", "back"]]
    : [["↑↓", "move"], [DI.ret, "open"], ["tab", "actions"], ["esc", "back"]];
  return deskScene(dBar(field + head + errLine + `<div class="dlist" role="listbox" aria-label="Research" style="padding-top:8px">${rows}</div>` + dHints(hints)));
};
document.addEventListener("click", e => {
  const b = e.target.closest("[data-ract]");
  if (!b) return;
  const [a, v] = b.dataset.ract.split(":");
  S.d.rv = "";
  if (a === "go") { S.d.rs = "running"; S.d.rtab = "agents"; }
  else if (a === "stop") S.d.rs = "stopped";
  else if (a === "tab") S.d.rtab = v;
  else if (a === "agent" || a === "room") {
    if (S.d.rs === "plan") return;
    const id = a === "room" ? "room" : v;
    if (!S.d.ropen.includes(id)) S.d.ropen.push(id);
    S.d.rtab = id;
  } else if (a === "close") { S.d.ropen = S.d.ropen.filter(id => id !== v); if (S.d.rtab === v) S.d.rtab = "agents"; }
  else if (a === "rounds") S.d.rmenu = !S.d.rmenu;
  else if (a === "round") { S.d.rround = Number(v); S.d.rmenu = false; }
  if (a !== "rounds") S.d.rmenu = false;
  render();
}, true);
DESK_SCREENS.splice(2, 0, { group: "Research", items: [["dResearches", "Research list", "ResearchView"], ["dResearchStart", "Start from the bar", "PromptBar, Cmd-R"]] });
VARIANTS.dResearches = [["List", "list"], ["New", "new"], ["Actions", "actions"], ["Could not load", "error"], ["Empty", "empty"]].map(([l, v]) => [l, () => { S.d.rl = v; render(); }, () => S.d.rl === v]);
// Quoting: select text in the paper, notes, sources or a thread and a Quote button floats over it (or press
// Cmd-'). It adds the text to the message as a > quote and puts the cursor under it, so several quotes with
// replies build one message. Exporting shows on the button, then a note over the workspace.
const RX = { quote: { rs: "done", rquote: true, rexport: "" }, exporting: { rs: "done", rquote: false, rexport: "busy" }, exported: { rs: "done", rquote: false, rexport: "saved" } };
// An opened research is a screen of the command center (Research, Opened). Its variants set where the research stands,
// which side pane tab shows and which agent threads are open as tabs.
const ROPEN = [["Plan", "plan"], ["Running", "running"], ["Agent thread", "thread"], ["Room", "room"], ["Done", "done"], ["Sources", "sources"], ["Reviewing", "reviewing"], ["Writing", "writing"], ["Follow-up", "followup"], ["Failed", "failed"], ["Stopped", "stopped"], ["Interrupted", "interrupted"], ["Quoting", "quote"], ["Exporting", "exporting"], ["Exported", "exported"]];
const rOpenState = v => {
  const base = RX[v] ?? { rs: v, rquote: false, rexport: "" };
  if (v === "thread") return { ...base, rs: "running", ropen: ["a"], rtab: "a" };
  if (v === "room") return { ...base, rs: "running", ropen: ["room"], rtab: "room" };
  if (v === "sources") return { ...base, rs: "done", ropen: [], rtab: "sources" };
  const tab = ["plan", "running", "reviewing", "writing", "followup"].includes(v) ? "agents" : "draft";
  return { ...base, ropen: [], rtab: tab };
};
