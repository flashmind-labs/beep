// ---- Desktop: Command center (CommandCenterWindow, Home) ----
// The full window behind the bar: every linked machine, the sessions running on them, what needs an answer
// and the runs Beep starts on its own. Variants: no machines, one machine offline, and a server too old to
// report machines and sessions (only chats, approvals and Coming up show).
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .cc-win { position: absolute; top: 34px; left: 12px; right: 12px; bottom: 12px; z-index: 3; display: grid; grid-template-columns: 184px minmax(0, 1fr);
  border-radius: 12px; overflow: hidden; border: 1px solid var(--d-border); box-shadow: 0 28px 70px rgba(0,0,0,.55);
  background: var(--d-ink); animation: barIn .22s var(--ease) }
.mac .cc-side { display: flex; flex-direction: column; gap: 12px; padding: 10px 8px; min-width: 0; border-right: 1px solid var(--d-border); background: rgba(15,16,17,.55); overflow-y: auto; scrollbar-width: thin; scrollbar-color: rgba(255,255,255,.18) transparent; scrollbar-gutter: stable; min-height: 0 }
.mac .cc-lights { display: flex; gap: 8px; padding: 2px 6px 2px }
.mac .cc-lights i { width: 11px; height: 11px; border-radius: 50%; background: #34363C }
.mac .cc-brand { display: flex; align-items: center; gap: 10px; padding: 0 6px }
.mac .cc-brand b { font-family: var(--display); font-weight: 600; font-size: 18px; letter-spacing: -0.03em }
.mac .cc-grp { display: grid; gap: 1px }
.mac .cc-grp h3 { margin: 0 0 3px; padding: 0 8px; font-size: 12px; font-weight: 600; color: var(--d-dim) }
.mac .cc-si { display: flex; align-items: center; gap: 8px; padding: 5px 8px; border-radius: 7px; border: 0; background: transparent; text-align: left; width: 100%; font-size: 13px; color: rgba(227,228,230,.86); white-space: nowrap }
.mac .cc-si:hover { background: rgba(227,228,230,.06) }
.mac .cc-si[aria-current="true"] { background: rgba(94,106,210,.26); color: var(--d-fg) }
.mac .cc-si svg { flex: none; color: var(--d-muted) }
.mac .cc-si .n { margin-left: auto; font-size: 11px; color: var(--d-muted); font-variant-numeric: tabular-nums }
.mac .cc-si .n.hot { color: var(--d-ink); background: var(--d-coral); border-radius: 999px; padding: 0 6px; font-weight: 600 }
.mac .cc-pdot { width: 8px; height: 8px; border-radius: 3px; flex: none }
.mac .ddot.busy { background: #4cb782 }
.mac .cc-foot { margin-top: auto; padding: 8px; font-size: 11.5px; line-height: 16px; color: var(--d-muted) }
.mac .cc-foot b { color: var(--d-fg); font-weight: 500 }
.mac .cc-main { display: flex; flex-direction: column; min-width: 0; min-height: 0; container-type: inline-size }
.mac .cc-top { flex: none; display: flex; align-items: center; gap: 10px; padding: 10px 16px; border-bottom: 1px solid var(--d-border) }
.mac .cc-top h2 { margin: 0; font-family: var(--display); font-weight: 600; font-size: 20px; letter-spacing: -0.03em; line-height: 1 }
.mac .cc-top small { font-size: 12px; color: var(--d-muted) }
.mac .cc-view { flex: 1; min-height: 0; overflow: auto; padding: 14px 16px 16px; display: flex; flex-direction: column; gap: 14px; scrollbar-width: thin }
.mac .cc-sec { display: flex; align-items: baseline; justify-content: space-between; gap: 10px; margin-bottom: 7px }
.mac .cc-sec h4 { margin: 0; font-size: 12px; font-weight: 600; color: var(--d-dim) }
.mac .cc-sec small { font-size: 11.5px; color: var(--d-muted) }
.mac .cc-panel { border: 1px solid var(--d-border); border-radius: 8px; background: color-mix(in srgb, var(--d-surface) 82%, transparent); min-width: 0 }
.mac .cc-cols { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 14px; align-items: start }
.mac .cc-wide { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 14px 20px; align-items: start }
.mac .cc-wide > div { display: flex; flex-direction: column; gap: 14px; min-width: 0 }
.mac .cc-main { --colw: clamp(720px, 60cqw, 960px) }
/* Home machines as a table: one row per machine, columns line up so a glance compares them. */
.mac .cc-mt { display: grid; grid-template-columns: minmax(110px, 1.2fr) 52px 92px 92px max-content minmax(0, 1.5fr) minmax(0, 1.1fr); font-size: 12px; border: 1px solid var(--d-border); border-radius: 8px; background: rgba(21,22,23,.82); overflow: hidden }
.mac .cc-mt > * { padding: 6px 10px; display: flex; align-items: center; gap: 6px; min-width: 0; border-top: 1px solid rgba(255,255,255,.05) }
.mac .cc-mt > .th { font-size: 12px; color: var(--d-dim); border-top: 0; padding-top: 7px; padding-bottom: 5px }
.mac .cc-mt .nm { flex-direction: column; align-items: flex-start; gap: 3px }
.mac .cc-mt .nm b { font-weight: 500; font-size: 12.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-mt .nm span { font-size: 10.5px; color: var(--d-muted); white-space: nowrap }
.mac .cc-mt .num { font-variant-numeric: tabular-nums; color: var(--d-muted); white-space: nowrap }
.mac .cc-mt .num.low { color: #f2a65f }
.mac .cc-mt .bar { flex: 1; height: 3px; border-radius: 2px; background: rgba(255,255,255,.07); overflow: hidden; max-width: 54px }
.mac .cc-mt .bar i { display: block; height: 100%; background: rgba(76,183,130,.7) }
.mac .cc-mt .caps { flex-wrap: nowrap; overflow: hidden; color: var(--d-muted); white-space: nowrap; text-overflow: ellipsis; display: block; line-height: 30px }
.mac .cc-mt .off { opacity: .5 }
/* Calm Home: no boxes, one line per thing, details on click. */
.mac .cc-calm { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 22px 26px; align-items: start }
.mac .cc-calm > div { display: grid; grid-template-columns: minmax(0, 1fr); gap: 22px; min-width: 0 }
.mac .cc-calm > div > div { min-width: 0 }
.mac .cc-calm h4 { margin: 0 0 4px; font-size: 11.5px; font-weight: 500; color: var(--d-dim); display: flex; gap: 6px; align-items: baseline }
.mac .cc-calm h4 .n { color: var(--d-muted); font-variant-numeric: tabular-nums }
.mac .cc-calm h4 .act { margin-left: auto; font-size: 11.5px; color: var(--d-muted); background: none; border: 0; padding: 0; cursor: pointer }
.mac .cc-l { all: unset; box-sizing: border-box; width: 100%; display: flex; align-items: center; gap: 10px; padding: 6px 2px; font-size: 12.5px; cursor: pointer; min-width: 0; border-radius: 6px }
.mac .cc-l:hover { background: rgba(255,255,255,.04) }
.mac .cc-l .x { flex: 1; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-l .x i { font-style: normal; color: var(--d-muted) }
.mac .cc-l .m { color: var(--d-muted); font-size: 11.5px; font-variant-numeric: tabular-nums; white-space: nowrap }
.mac .cc-l .ic { width: 18px; display: grid; place-items: center; color: var(--d-muted); flex: none; font-size: 10px; font-weight: 600 }
.mac .cc-l .ok { line-height: 16px; flex: none; font-size: 11.5px; padding: 2px 9px; border-radius: 6px; border: 1px solid rgba(76,183,130,.35); color: rgb(111,211,168); background: none }
.mac .cc-l.dim { opacity: .45 }
.mac .cc-mach { padding: 10px 12px; display: grid; gap: 6px; align-content: start; text-align: left; border: 1px solid var(--d-border); border-radius: 8px; background: rgba(21,22,23,.82); color: var(--d-fg) }
.mac .cc-mach:hover { border-color: #34363C }
.mac .cc-mach.off { opacity: .6 }
.mac .cc-mach .h { display: flex; align-items: center; gap: 8px; min-width: 0 }
.mac .cc-mach .h b { font-weight: 500; font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-mach .os { font-size: 11.5px; color: var(--d-muted) }
.mac .cc-caps { display: flex; flex-wrap: wrap; gap: 4px }
.mac .cc-cap { font-size: 10.5px; line-height: 16px; padding: 0 6px; border-radius: 5px; background: var(--d-elev); border: 1px solid var(--d-border); color: rgba(227,228,230,.82); white-space: nowrap }
.mac .cc-meter { height: 4px; border-radius: 4px; background: var(--d-elev); overflow: hidden }
.mac .cc-meter i { display: block; height: 100%; border-radius: 4px; background: var(--d-emerald) }
.mac .cc-mach .st { display: flex; justify-content: space-between; gap: 6px; font-size: 11px; color: var(--d-muted); font-variant-numeric: tabular-nums }
.mac .cc-row { display: grid; grid-template-columns: 28px minmax(0, 1fr) auto; gap: 10px; align-items: center; padding: 8px 12px; text-align: left; width: 100%; border: 0; background: transparent; color: var(--d-fg) }
.mac .cc-row + .cc-row { border-top: 1px solid var(--d-border) }
.mac button.cc-row:hover { background: rgba(227,228,230,.04) }
.mac .cc-row .t { display: grid; min-width: 0; line-height: 18px }
.mac .cc-row .t b { font-weight: 500; font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-row .t span { font-size: 11.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-row .r { font-size: 11.5px; color: var(--d-muted); display: grid; justify-items: end; gap: 2px; font-variant-numeric: tabular-nums }
/* Home's open PRs and issues by project: counts and chips sit on one line. */
.mac .cc-row.cc-gr .r { display: flex; align-items: center; gap: 6px; justify-items: initial }
.mac .cc-row.cc-gr.it { padding-top: 5px; padding-bottom: 5px; grid-template-columns: 16px minmax(0, 1fr) auto }
.mac .cc-row.cc-gr.it + .cc-row.cc-gr.it { border-top-color: rgba(255,255,255,.04) }
.mac .cc-ag { width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; font-size: 10.5px; font-weight: 600; border: 1px solid var(--d-border); background: var(--d-elev) }
.mac .cc-ag.claude { color: #F2A27A } .mac .cc-ag.codex { color: #8A9CF0 }
.mac .cc-ag.beep { color: var(--d-muted) }
.mac .cc-tag { display: inline-flex; align-items: center; gap: 6px; font-size: 10.5px; font-weight: 500; padding: 0 7px; line-height: 17px; border-radius: 999px; border: 1px solid var(--d-border); color: var(--d-muted); white-space: nowrap }
.mac .cc-tag.run { color: var(--d-emerald); border-color: rgba(94,106,210,.45) }
.mac .cc-tag.wait { color: #F2C94C; border-color: rgba(242,201,76,.45) }
.mac .cc-tag.term { color: var(--d-fg) }
.mac .cc-tag.bad { color: var(--d-err); border-color: rgba(235,87,87,.45) }
.mac .cc-tag .dspin { width: 9px; height: 9px }
.mac .cc-ask { padding: 11px; display: grid; gap: 8px }
.mac .cc-ask + .cc-ask { border-top: 1px solid var(--d-border) }
.mac .cc-ask .h { display: flex; gap: 10px; align-items: center }
.mac .cc-ask .h .t { display: grid; line-height: 18px; min-width: 0 }
.mac .cc-ask .h b { font-weight: 500; font-size: 13px }
.mac .cc-ask .h .t span { font-size: 11.5px; color: var(--d-muted) }
.mac .cc-ask .kv { display: grid; grid-template-columns: 52px 1fr; gap: 2px 8px; padding: 7px 9px; border-radius: 8px; background: var(--d-ink); border: 1px solid var(--d-border); font-size: 12px }
.mac .cc-ask .kv span:nth-child(odd) { color: var(--d-muted) }
.mac .cc-ask .acts { display: flex; flex-wrap: wrap; gap: 6px }
.mac .cc-ask .ok { font-size: 12px; color: var(--d-emerald) }
.mac .cc-when { width: 58px; text-align: center; font-size: 10.5px; font-weight: 600; color: var(--d-muted); line-height: 13px; font-variant-numeric: tabular-nums }
.mac .cc-when b { display: block; font-size: 12.5px; color: var(--d-fg) }
.mac .cc-note { display: flex; gap: 10px; align-items: center; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--d-border); background: var(--d-elev); font-size: 12.5px; line-height: 18px; color: rgba(227,228,230,.9) }
.mac .cc-note .ddot { margin-top: 1px }
.mac .cc-empty { padding: 22px 18px; display: grid; justify-items: center; gap: 8px; text-align: center; font-size: 13px; line-height: 19px; color: var(--d-muted) }
.mac .cc-empty b { color: var(--d-fg); font-size: 14px; font-weight: 500 }
.mac .cc-empty ol { margin: 0; padding-left: 18px; text-align: left; display: grid; gap: 3px; color: rgba(227,228,230,.86) }
.mac .cc-empty code { font-size: 12px; background: var(--d-elev); border: 1px solid var(--d-border); border-radius: 4px; padding: 0 4px }
.mac .cc-comp { flex: none; margin: 0 16px 14px; padding: 6px 6px 6px 14px; border-radius: 12px; border: 1px solid var(--d-border); background: rgba(28,29,31,.92); display: flex; align-items: center; gap: 10px }
.mac .cc-comp .in { flex: 1; min-width: 0; font-size: 14px; color: var(--d-fg); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-comp .in .ph { color: var(--d-dim) }
.mac .cc-comp .in .caret { height: 15px }
/* The input bars (Home, the project agent) stay at most 760px wide, centered; narrower windows keep the 16px gutter. */
.mac .cc-comp { margin-inline: max(16px, calc((100% - 760px) / 2)) }
@container (max-width: 520px) {
  .mac .cc-cols { grid-template-columns: minmax(0, 1fr) }
}
@media (max-width: 600px) {
  .mac .cc-win { top: 30px; left: 4px; right: 4px; bottom: 4px; grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr) }
  .mac .cc-side { flex-direction: row; gap: 4px; padding: 8px; border-right: 0; border-bottom: 1px solid var(--d-border); overflow-x: auto; overflow-y: hidden }
  .mac .cc-lights, .mac .cc-brand, .mac .cc-foot, .mac .cc-grp.machines, .mac .cc-grp h3 { display: none }
  .mac .cc-grp { display: flex; gap: 2px }
  .mac .cc-si { width: auto }
  .mac .cc-top { padding: 10px 12px } .mac .cc-top small { display: none } .mac .cc-view { padding: 12px } .mac .cc-comp { margin: 0 12px 12px }
  .mac .cc-mach .st { flex-wrap: wrap }
}`;
  document.head.append(st);
})();

const CC_IC = {
  home: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 11l9-7 9 7v9a1 1 0 01-1 1h-5v-6H9v6H4a1 1 0 01-1-1z"/></svg>',
  run: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M10 8l6 4-6 4z" fill="currentColor"/></svg>',
  cal: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="3" y="5" width="18" height="16" rx="2"/><path d="M3 10h18M8 3v4M16 3v4"/></svg>',
  chat: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H8l-4 4z"/></svg>',
  mem: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 3a6 6 0 00-6 6c0 2.5 1.5 3.8 2.5 5 .7.8 1 1.6 1 2.5V18h5v-1.5c0-.9.3-1.7 1-2.5 1-1.2 2.5-2.5 2.5-5a6 6 0 00-6-6zM10 21h4"/></svg>',
};
// st: "busy" (agents running), "on" or "off".
const CC_MACHINES = [
  { name: "Build box", os: "Linux, 32 cores", caps: ["GPU", "CUDA", "Docker"], agents: 3, load: 41, stat: "41% CPU", st: "busy" },
  { name: "Mac Studio", os: "macOS", caps: ["Xcode", "iOS simulator", "Android emulator"], agents: 1, load: 22, stat: "22% CPU", st: "on" },
  { name: "Windows PC", os: "Windows", caps: ["GPU", "CUDA", "Blender", "Unreal"], agents: 0, load: 6, stat: "6% GPU", st: "on" },
  { name: "nas-box", os: "Linux", caps: ["Storage", "Docker"], agents: 0, load: 12, stat: "2.1 TB free", st: "on" },
  { name: "Phone", os: "Android", caps: ["Camera", "Location"], agents: 0, load: 0, stat: "12 min ago", st: "off", idle: "offline" },
];
// Sessions: agent, title, project, how it started, machine, status html, and
// how long since its last step or reply (`at`, minutes), which orders them.
const CC_RUNNING = [
  { ag: "claude", ini: "CC", t: "Swipeable approval strip on iOS", sub: "beep-app, Claude Code", repo: "acme/beep-app", on: "Build box", tag: '<span class="cc-tag wait">needs you</span>', age: "1 min", at: 1 },
  { ag: "codex", ini: "CX", t: "Fix the device build script", sub: "beep-app, Codex", repo: "acme/beep-app", on: "Mac Studio", tag: '<span class="cc-tag run"><span class="dspin"></span>xcodebuild</span>', age: "4 min", at: 4 },
  { ag: "claude", ini: "CC", t: "Clippy fixes in the sync crate", sub: "mesh-relay, Claude Code", repo: "acme/platform/mesh-relay", on: "Build box", tag: '<span class="cc-tag term">in terminal</span>', age: "31 min", at: 31, term: true },
  { ag: "beep", ini: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/></svg>', t: "Weekly inbox digest", sub: "Beep routine", on: "Build box", tag: '<span class="cc-tag run"><span class="dspin"></span>reading mail</span>', age: "2 min", at: 2 },
];
const CC_COMING = [
  ["02:00", "nightly", "Full test suite and clippy", "beep-app, Codex on Build box, posts a summary to Chats", "every day"],
  ["07:30", "Mon", "Render a turntable of the new tree set", "Hollow Pines, Claude Code with Blender on Windows PC", "weekly"],
  ["09:00", "Mon", "Morning brief", "Beep routine, read aloud on this Mac", "weekdays"],
];

D.dHome = () => {
  const v = S.d.cc || "default";
  const old = v === "old", empty = v === "empty", offline = v === "offline";
  const conn = v === "connecting" || v === "stuck";
  if (conn) return mbar() + ccConnecting(v === "stuck");
  const machines = empty || old ? [] : CC_MACHINES.map(m => offline && m.name === "Mac Studio" ? { ...m, st: "off", agents: 0, load: 0, stat: "9 min ago", idle: "offline" } : m);
  const gone = offline ? "Mac Studio" : "";
  const running = empty || old ? [] : CC_RUNNING.filter(r => r.on !== gone).sort((a, b) => a.at - b.at);
  const live = machines.filter(m => m.st !== "off").length;
  const needs = 3;

  const si = (ic, label, n, cur, hot) => `<button class="cc-si"${cur ? ' aria-current="true"' : ` data-toast="${label}"`}>${ic}${label}${n ? `<span class="n${hot ? " hot" : ""}">${n}</span>` : ""}</button>`;
  const side = `<aside class="cc-side">
      <div class="cc-lights"><i></i><i></i><i></i></div>
      <div class="cc-brand"><b>Beep</b></div>
      <div class="cc-grp"><h3>Pilot</h3>
        ${si(CC_IC.home, "Home", needs, true, true)}
        ${old ? "" : si(CC_IC.run, "Running", running.length || "")}
        ${old || empty ? "" : si(CC_IC.sync, "Sync", 3)}
        ${si(CC_IC.cal, "Coming up", CC_COMING.length)}
        ${si(CC_IC.chat, "Chats", "")}
        ${si(CC_IC.mem, "Memories", "")}
      </div>
      ${old || empty ? "" : `<div class="cc-grp"><h3>Projects</h3>
        <button class="cc-si" data-toast="beep-app: 3 sessions"><span class="cc-pdot" style="background:var(--d-coral)"></span>beep-app<span class="n">${offline ? 1 : 2}</span></button>
        <button class="cc-si" data-toast="mesh-relay: 1 session"><span class="cc-pdot" style="background:#6FA8DC"></span>mesh-relay<span class="n">1</span></button>
        <button class="cc-si" data-toast="Hollow Pines: Blender work on Windows PC"><span class="cc-pdot" style="background:#B18CE0"></span>Hollow Pines</button>
      </div>`}
      ${old || empty ? "" : `<div class="cc-grp machines"><h3>Machines<button aria-label="Add a machine" data-dgo="dAddMachine">${CC_IC.plus}</button></h3>${machines.map(m => `<button class="cc-si" data-toast="${m.name}: ${m.st === "off" ? m.idle : m.agents ? m.agents + " running" : "idle"}"><span class="ddot${m.st === "busy" ? " busy" : m.st === "off" ? " off" : ""}"></span>${m.name}${m.agents ? `<span class="n">${m.agents}</span>` : ""}</button>`).join("")}${tmItem()}</div>`}
      <div class="cc-foot">${old ? "<span>Machines unavailable</span>"
        : empty ? "<span>No machines linked</span>"
        : `<span><span class="ddot"></span> <b>${live}</b> of ${machines.length} machines online</span>`}</div>
    </aside>`;

  // File sync shows only while it runs: a hairline bar and one muted line.
  const syncing = { "Mac Studio": ["142 of 610 files", 23, "about 1 min"] };
  const syncLine = m => syncing[m.name] && m.st !== "off" ? `<div style="display:grid;gap:3px;margin-top:2px"><div style="height:2px;border-radius:1px;background:rgba(255,255,255,.06);overflow:hidden"><i style="display:block;height:100%;width:${syncing[m.name][1]}%;background:rgba(76,183,130,.6)"></i></div><span style="font-size:10.5px;color:var(--d-muted)">Syncing acme/widget · ${syncing[m.name][0]} · ${syncing[m.name][2]}</span></div>` : "";
  const mcard = m => `<button class="cc-mach${m.st === "off" ? " off" : ""}" data-toast="${m.name}: ${m.caps.join(", ")}">
      <div class="h"><span class="ddot${m.st === "busy" ? " busy" : m.st === "off" ? " off" : ""}"></span><b>${m.name}</b></div>
      <span class="os">${m.os}</span>
      <div class="cc-caps">${m.caps.map(c => `<span class="cc-cap">${c}</span>`).join("")}</div>
      <div class="cc-meter"><i style="width:${m.load}%"></i></div>
      <div class="st"><span>${m.st === "off" ? m.idle : m.agents ? m.agents + (m.agents === 1 ? " agent" : " agents") : "idle"}</span><span>${m.stat}</span></div>${syncLine(m)}</button>`;
  const mrow = m => {
    const off = m.st === "off" ? " off" : "", sy = syncing[m.name] && m.st !== "off" ? syncing[m.name] : null;
    // Free and total disk, used and total memory, in GB, as the node reports them.
    const stats = [[212, 931, 9.1, 32], [38, 1863, 41.5, 64], [7, 460, 18.2, 36], [120, 512, 5.4, 16]];
    const st = stats[machines.indexOf(m) % stats.length], dk = st && [st[0], st[1]], mm = st && [st[2], st[3]];
    const now = m.st === "off" ? m.idle
      : sy ? `<span style="display:grid;gap:3px;width:100%"><span class="bar" style="max-width:none"><i style="width:${sy[1]}%"></i></span><span style="font-size:10.5px;white-space:nowrap">Syncing, ${sy[1]}%</span></span>`
      : m.agents ? "working" : "idle";
    return `<button class="nm${off}" style="all:unset;padding:6px 10px;display:flex;flex-direction:column;gap:0;cursor:pointer;border-top:1px solid rgba(255,255,255,.05)" data-toast="${m.name}"><b><span class="ddot${m.st === "busy" ? " busy" : off}" style="margin-right:6px"></span>${m.name}</b><span>${m.os}</span></button>
      <span class="num${off}">${m.agents || "–"}</span>
      <span class="num${off}${dk && dk[0] < 0.1 * dk[1] ? " low" : ""}" title="${dk ? `Workspace: ${dk[0]} GB free of ${dk[1]} GB` : ""}">${m.st === "off" || !dk ? "–" : dk[0] + " GB free"}</span>
      <span class="num${off}" title="${mm ? `Load ${(m.load / 25).toFixed(2)} on 8 cores` : ""}">${m.st === "off" || !mm ? "–" : `${mm[0]} / ${mm[1]} GB`}</span>
      <span class="num${off}" title="Link 508 MB · File sync 40 MB · Transfers 17 GB">14 GB / 3.2 GB</span>
      <span class="caps${off}" title="${m.caps.join(", ")}">${m.caps.join(" · ")}</span>
      <span class="num${off}">${now}</span>`;
  };
  const machSec = old ? "" : `<div><div class="cc-sec"><h4>Machines</h4><small>${empty ? "None linked" : `${live} of ${machines.length} online`}<button class="dbtn" style="margin-left:10px;padding:3px 10px;font-size:12px" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button></small></div>
      ${empty ? `<div class="cc-panel cc-empty"><b>No machines linked yet</b>
          <span style="max-width:52ch">Link a computer to run coding agents and commands on it.</span>
          <span>Beep gives you a one-time command; run it on the computer and it links itself to your account.</span>
          <button class="dbtn pri" data-dgo="dAddMachine">Add a machine</button></div>`
        : `<div class="cc-mt" role="table"><span class="th">Machine</span><span class="th">Agents</span><span class="th">Disk</span><span class="th">Memory</span><span class="th">Sent / received</span><span class="th">Can run</span><span class="th">Now</span>
            ${machines.map(mrow).join("")}${tmCells()}</div>`}</div>`;
  const offNote = offline ? `<div class="cc-note"><span class="ddot bad"></span><span><b>Mac Studio is offline.</b> Its session left Running now. It comes back when beep-node runs there again.</span></div>` : "";

  const rrow = r => `<button class="cc-row" data-dact="dset:ags:${r.term ? "outside" : r.tag.includes("wait") ? "asks" : "working"}" data-dgo="dAgentSession"><span class="cc-ag ${r.ag}">${r.ini}</span>
      <span class="t"><b>${r.t}</b><span>${r.sub} on ${r.on}</span></span><span class="r">${r.tag}${r.age}</span></button>`;
  const runSec = old ? "" : `<div><div class="cc-sec"><h4>Running now</h4><small>${running.length ? `${running.length} sessions on ${(n => n + (n === 1 ? " machine" : " machines"))(new Set(running.map(r => r.on)).size)}` : "Nothing running"}</small></div>
      ${running.length ? `<div class="cc-panel">${running.map(rrow).join("")}</div>`
        : `<div class="cc-panel cc-empty" style="padding:16px">Sessions you start here, in the bar or in a terminal show up once a machine is linked.</div>`}</div>`;

  const ask = (ag, ini, title, sub, body, acts) => `<div class="cc-ask"><div class="h"><span class="cc-ag ${ag}">${ini}</span><span class="t"><b>${title}</b><span>${sub}</span></span></div>${body}<div class="acts">${acts}</div></div>`;
  const needSec = `<div><div class="cc-sec"><h4>Needs you</h4><small>3 waiting</small></div><div class="cc-panel">
      ${ask("beep", '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>', "Send this email?", "Chat: Deposit for the flat, Gmail", `<div class="kv"><span>To</span><span>landlord@example.com</span><span>Subject</span><span>Deposit return</span></div>`,
        `<button class="dbtn pri" data-cc="ok:Sent">Allow</button><button class="dbtn" data-cc="ok:Allowed, and from now on for Gmail">Always allow</button><button class="dbtn" data-cc="ok:Denied">Deny</button>`)}
      ${ask("claude", "CC", "Keep the old endpoint or remove it?", "Swipeable approval strip on iOS, Build box", `<div style="font-size:12.5px;line-height:18px;color:rgba(227,228,230,.9)">Older phones still call it. Removing it is cleaner, keeping it is safer until they update.</div>`,
        `<button class="dbtn pri" data-cc="ok:Answered: keep it">Keep it</button><button class="dbtn" data-cc="ok:Answered: remove it">Remove it</button><button class="dbtn" data-toast="Opens the session to reply">Reply</button>`)}
      ${ask("beep", CC_IC.mem, "Add to your instructions?", "Beep suggests · said in 3 chats", inDiff([["add", "- Never post on GitHub yourself."]]) + `<div class="in-ev">${inChip(CC_IC.chat, "said in 3 chats", "Opens the 3 chats")}${inChip(CC_IC.mem, "1 memory", "Opens the memory")}</div>`,
        `<button class="dbtn pri" data-cc="ok:Accepted, saved as version 8">Accept</button><button class="dbtn" data-cc="ok:Dismissed">Dismiss</button>`)}
    </div></div>`;
  const updSec = `<div><div class="cc-sec"><h4>Updates</h4><small>From your apps<button class="dbtn quiet" style="margin-left:10px;padding:3px 10px;font-size:12px" data-toast="All dismissed">Clear</button></small></div><div class="cc-panel">
      ${UPDATES.slice(0, 3).map(([l, app, t, w]) => `<div class="cc-row"><span class="cc-ag beep" style="font-size:11px;font-weight:600">${l}</span><button class="t" style="all:unset;flex:1;min-width:0;cursor:pointer;display:grid" data-dact="dset:wc:update" data-dgo="dWinChat"><b style="font-weight:500;white-space:normal;line-height:17px">${t}</b><span>${app} · ${w}</span></button><span class="r"><button class="dbtn quiet" style="padding:2px 6px" aria-label="Dismiss" data-toast="Dismissed">✕</button></span></div>`).join("")}
    </div></div>`;
  const comingSec = `<div><div class="cc-sec"><h4>Coming up</h4><small>Runs Beep starts on its own</small></div><div class="cc-panel">
      ${CC_COMING.map(([t, d, b, s, r]) => `<div class="cc-row"><span class="cc-when"><b>${t}</b>${d}</span><span class="t"><b>${b}</b><span>${s}</span></span><span class="r"><span class="cc-tag">${r}</span></span></div>`).join("")}
    </div></div>`;

  // Calm Home: Machines and Running on the left, what wants a look on the right. One line each.
  const L = (ic, x, m, attrs = "", cls = "") => `<button class="cc-l${cls}" ${attrs}><span class="ic">${ic}</span><span class="x">${x}</span>${m}</button>`;
  const H = (t, n, act = "") => `<h4>${t}${n ? `<span class="n">${n}</span>` : ""}${act}</h4>`;
  const calmMach = old ? "" : `<div>${H("Machines", empty ? "" : `${live}/${machines.length}`, `<button class="act" data-dgo="dAddMachine">+ Add</button>`)}
      ${empty ? L("+", "Add a machine to run agents on it", "", 'data-dgo="dAddMachine"')
        : machines.map(m => { const sy = syncing[m.name] && m.st !== "off" ? syncing[m.name] : null;
            return L(`<span class="ddot${m.st === "busy" ? " busy" : m.st === "off" ? " off" : ""}"></span>`, m.name,
              `<span class="m">${m.st === "off" ? m.idle : [sy ? `syncing ${sy[1]}%` : m.agents ? m.agents + (m.agents === 1 ? " agent" : " agents") : "", m.load + "%"].filter(Boolean).join(" · ")}</span>`,
              `data-toast="${m.name}: ${m.os}, ${m.caps.join(", ")}"`, m.st === "off" ? " dim" : ""); }).join("")}</div>`;
  const calmRun = old || !running.length ? "" : `<div>${H("Running", running.length)}
      ${running.map(r => L(`<span class="cc-ag ${r.ag}" style="width:18px;height:18px;font-size:8.5px;border-radius:5px">${r.ini}</span>`, `${r.t} <i>· ${r.on}</i>`,
        `<span class="m">${r.tag.includes("wait") ? '<span style="color:var(--d-amber,#E8B04B)">needs you</span>' : r.age}</span>`, 'data-dgo="dAgentSession"')).join("")}</div>`;
  const calmNeed = `<div>${H("Needs you", 3)}
      ${L(CC_IC.chat, "Send this email? <i>· Gmail</i>", '<span class="ok" data-toast="Sent">Allow</span>', 'data-toast="Opens the full request: To, Subject, body"')}
      ${L('<span class="cc-ag claude" style="width:18px;height:18px;font-size:8.5px;border-radius:5px">CC</span>', "Keep the old endpoint or remove it?", '<span class="m">answer</span>', 'data-dgo="dAgentSession"')}
      ${L(CC_IC.mem, "Add to your instructions? <i>· said in 3 chats</i>", '<span class="ok" data-toast="Accepted">Accept</span>', 'data-toast="Opens the suggested change"')}</div>`;
  const calmUpd = `<div>${H("Updates", UPDATES.length, '<button class="act" data-toast="All dismissed">Clear</button>')}
      ${UPDATES.slice(0, 3).map(([l, app, t, w]) => L(l, `${t} <i>· ${app}</i>`, `<span class="m">${w}</span>`, 'data-toast="Opens a chat about this update"')).join("")}</div>`;
  const calmComing = `<div>${H("Coming up")}
      ${CC_COMING.map(([t, d, b]) => L("", b, `<span class="m">${d === "nightly" ? "" : d + " "}${t}</span>`, 'data-toast="Opens the routine"')).join("")}</div>`;
  // The machines keep their table; everything else stays one line per thing.
  const calmTable = old ? "" : `<div>${H("Machines", empty ? "" : `${live}/${machines.length}`, `<button class="act" data-dgo="dAddMachine">+ Add</button>`)}${empty ? L("+", "Add a machine to run agents on it", "", 'data-dgo="dAddMachine"') : machSec.replace(/^<div><div class="cc-sec">[\s\S]*?<\/div>\s*/, "<div>")}</div>`;
  const calm = `${offNote}<div class="cc-calm" style="grid-template-columns:minmax(0,1fr)"><div>${calmTable}</div></div><div class="cc-calm"><div>${calmRun}${calmComing}</div><div>${calmNeed}${calmUpd}</div></div>`;
  // Balanced Home: the detailed boxes, minus subtitles, tags and expanded cards.
  // Each row is a title and one muted line; detail opens on click.
  const sec = (t, n, act = "") => `<div class="cc-sec"><h4>${t}${n ? ` <span style="color:var(--d-muted);font-weight:500;letter-spacing:0">${n}</span>` : ""}</h4>${act ? `<small>${act}</small>` : ""}</div>`;
  const brow = (ic, t, sub, r, attrs = "") => `<button class="cc-row" ${attrs}>${ic}<span class="t"><b>${t}</b><span>${sub}</span></span><span class="r">${r}</span></button>`;
  const tile = x => `<span class="cc-ag beep" style="font-size:11px;font-weight:600">${x}</span>`;
  const bMach = old ? "" : `<div>${sec("Machines", empty ? "" : `${live} of ${machines.length} online`, `<button class="dbtn" style="padding:3px 10px;font-size:12px" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button>`)}${empty ? `<div class="cc-panel cc-empty" style="padding:16px"><b>No machines linked yet</b><button class="dbtn pri" data-dgo="dAddMachine">Add a machine</button></div>` : machSec.replace(/^<div><div class="cc-sec">[\s\S]*?<\/div>\s*/, "<div>")}</div>`;
  const bRun = old ? "" : `<div>${sec("Running now", running.length || "")}<div class="cc-panel">${running.length ? running.map(r => brow(`<span class="cc-ag ${r.ag === "codex" ? "codex" : "claude"}" style="width:18px;height:18px" title="${r.ag === "codex" ? "Codex" : "Claude Code"}">${r.ag === "codex" ? "CX" : "CC"}</span>`, r.t, `${r.repo || r.sub.split(",")[0]} · ${r.on}`,
      r.tag.includes("wait") ? '<span class="cc-tag wait">needs you</span>' : r.age, 'data-dgo="dAgentSession" style="grid-template-columns:auto minmax(0,1fr) auto"')).join("") : '<div class="cc-empty" style="padding:14px">Nothing running</div>'}</div></div>`;
  const okb = (label, toast) => `<span class="dbtn pri" style="padding:2px 10px;font-size:12px;min-height:0;line-height:18px" data-toast="${toast}">${label}</span>`;
  const bNeed = `<div>${sec("Needs you", 3)}<div class="cc-panel">
      ${brow(tile(CC_IC.chat), "Send this email?", "Gmail · Deposit return to landlord@example.com", okb("Allow", "Sent"), 'data-toast="Opens the full email with Always allow and Deny"')}
      ${brow('<span class="cc-ag claude">CC</span>', "Keep the old endpoint or remove it?", "Claude Code · Swipeable approval strip", '<span class="dbtn" style="padding:2px 10px;font-size:12px;min-height:0;line-height:18px">Answer</span>', 'data-dgo="dAgentSession"')}
      ${brow(tile(CC_IC.mem), "Add to your instructions?", "“Never post on GitHub yourself.” · said in 3 chats", okb("Accept", "Accepted, saved as version 8"), 'data-toast="Opens the suggested change"')}
    </div></div>`;
  const bUpd = `<div>${sec("Updates", UPDATES.length, '<button class="dbtn quiet" style="padding:3px 10px;font-size:12px" data-toast="All dismissed">Clear</button>')}<div class="cc-panel">
      ${UPDATES.slice(0, 3).map(([l, app, t, w]) => brow(tile(l), t, `${app} · ${w}`, '<span class="dbtn quiet" style="padding:2px 6px" data-toast="Dismissed">✕</span>', 'data-toast="Opens a chat about this update"')).join("")}</div></div>`;
  const bComing = `<div>${sec("Coming up")}<div class="cc-panel">
      ${CC_COMING.map(([t, d, b, sub, r]) => brow(`<span class="cc-when"><b>${t}</b>${d}</span>`, b, sub.split(",")[0] + " · " + r, "", 'data-toast="Opens the routine"')).join("")}</div></div>`;
  // Open PRs and issues, broken down by project: one row per repository with its counts (PRs, issues,
  // waiting on you), then its items as rows under it, newest first, three at most. The project row opens
  // the project's page (the repo in the browser when it is not imported); an item row opens the PR or
  // issue on GitHub or Forgejo. Review prefills Start a coding agent; Reply opens a chat with Beep.
  // Hidden without a git host credential; a host that fails says so above the rows.
  const gv = S.d.git || "default";
  const PR_IC = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="5" r="2.2"/><circle cx="6" cy="19" r="2.2"/><circle cx="18" cy="19" r="2.2"/><path d="M6 7.2v9.6M18 16.8V10a3 3 0 00-3-3h-4M13 4.5L10.5 7 13 9.5"/></svg>';
  const IS_IC = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="1.6" fill="currentColor"/></svg>';
  const chip = (cls, t) => `<span class="cc-tag${cls ? " " + cls : ""}">${t}</span>`;
  const gb = (label, toast) => `<span class="dbtn" style="padding:1px 9px;font-size:11.5px;min-height:0;line-height:17px" data-toast="${toast}">${label}</span>`;
  // Items: number, title, PR?, chips, age, waiting on you.
  const GIT = [
    { repo: "acme/widget", host: "github.com", imported: true, last: "40 min", items: [
      [144, "Add a dark theme to the onboarding flow", true, [chip("wait", "your review")], "40 min", true],
      [142, "Retry uploads when the token expires", true, [chip("wait", "changes requested"), chip("bad", "checks failing")], "2 h", true],
      [139, "Split the settings screen into tabs", true, [chip("", "draft")], "1 d", false],
      [131, "Crash when the camera permission is denied", false, [chip("", "assigned")], "2 d", false],
      [120, "Document the release steps", false, [], "12 d", false],
    ] },
    { repo: "tools-org/uploader", host: "git.example.com", imported: true, last: "5 h", items: [
      [61, "Drop the old v1 endpoints", true, [chip("wait", "your review"), chip("bad", "checks failing")], "5 h", true],
      [57, "Stream large files in chunks", true, [chip("run", "approved")], "3 d", false],
      [48, "Resume after the laptop sleeps", false, [], "6 d", false],
    ] },
    { repo: "acme/docs-site", host: "github.com", imported: false, last: "4 d", items: [
      [12, "Broken links on the pricing page", false, [], "4 d", false],
    ] },
  ];
  const projects = gv === "error" ? GIT.filter(g => g.host === "github.com") : GIT;
  const gitem = g => ([n, t, pr, chips, age]) => `<button class="cc-row cc-gr it" style="padding-left:30px" data-toast="Opens ${g.repo}#${n} on ${g.host}"><span style="color:var(--d-muted);display:grid">${pr ? PR_IC : IS_IC}</span>
      <span class="t" style="display:flex;gap:6px;align-items:baseline"><span style="color:var(--d-muted);font-variant-numeric:tabular-nums;flex:none">#${n}</span><b style="font-weight:450;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${t}</b></span>
      <span class="r">${chips.join("")}<span style="color:var(--d-muted);font-size:11.5px;min-width:40px;text-align:right">${age}</span>${pr ? gb("Review", "Opens Start a coding agent with: Review PR " + g.repo + "#" + n + ", post only after I approve") : ""}${gb("Reply", "Opens a chat: Help me reply to " + g.repo + "#" + n)}</span></button>`;
  const count = (n, one, many) => `${n} ${n === 1 ? one : many}`;
  const gproj = (g, i) => {
    const prs = g.items.filter(x => x[2]).length, iss = g.items.length - prs, wait = g.items.filter(x => x[5]).length;
    const more = g.items.length > 3 ? `<button class="cc-row cc-gr it" style="padding-left:30px;color:var(--d-muted);font-size:12px;display:block" data-toast="Shows all ${g.items.length} in ${g.repo}">Show all ${g.items.length}</button>` : "";
    return `<button class="cc-row cc-gr" style="border-top:${i ? "1px solid var(--d-border)" : "0"}" ${g.imported ? 'data-dgo="dProjectsPage"' : `data-toast="Opens ${g.repo} on ${g.host}"`}><span class="cc-pdot" style="background:${["#F2994A", "#6FA8DC", "#B18CE0"][i % 3]}"></span>
      <span class="t"${g.imported ? "" : ' title="Right-click: Import…, Import with settings…"'}><b>${g.repo}</b><span>${g.host}${g.imported ? "" : " · not imported"}</span></span>
      <span class="r" style="gap:12px;font-size:12px;color:var(--d-muted);font-variant-numeric:tabular-nums">${prs ? `<span>${count(prs, "PR", "PRs")}</span>` : ""}${iss ? `<span>${count(iss, "issue", "issues")}</span>` : ""}${wait ? `<span class="cc-tag wait">${wait} waiting on you</span>` : ""}<span style="min-width:40px;text-align:right">${g.last}</span>${g.imported ? "" : gb("Import", "Imports " + g.repo + " on build-box, into ~/Projects/" + g.repo + ". Right-click for Import… and Import with settings…")}</span></button>
      ${g.items.slice(0, 3).map(gitem(g)).join("")}${more}`;
  };
  const gerr = gv === "error" ? `<div class="cc-note" style="margin:8px 10px 2px"><span class="ddot bad"></span><span><b>git.example.com</b> did not answer: the request timed out. Its projects are left out until it does.</span></div>` : "";
  const gtotal = projects.reduce((a, g) => a + g.items.length, 0);
  const bGit = gv === "none" ? "" : `<div>${sec("Open on GitHub and Forgejo", `${gtotal} in ${projects.length} projects`)}<div class="cc-panel">${gerr}
      ${projects.map(gproj).join("")}
    </div></div>`;
  // Terminals on every machine, like a tmux session list: they keep running with the window closed.
  const tRow = ([n, where, st, note], i) => `<button class="cc-row" data-dgo="dTerminals" style="border-top:${i ? "1px solid var(--d-border)" : "0"}"><span class="ddot${st === "exit" ? " off" : ""}"></span>
      <span class="t"><b style="font-family:var(--mono,ui-monospace,monospace);font-weight:500">${n}</b><span>${where}</span></span><span class="r" style="font-size:12px;color:var(--d-muted)">${note}</span></button>`;
  const tMach = m => `<div class="cc-panel" style="padding:0"><div class="cc-row" style="cursor:default"><span class="t"><b>${m.name}</b><span>${m.terms.length ? `${m.terms.filter(t => t[2] !== "exit").length} running` : "No terminals"}</span></span>
      <span class="r"><button class="dbtn" data-dgo="dTerminals" style="padding:2px 8px">New</button></span></div>${m.terms.map((t, i) => tRow(t, i + 1)).join("")}</div>`;
  const bTerm = empty || old ? "" : `<div>${sec("Terminals", `${CC_TERMS.reduce((a, m) => a + m.terms.length, 0)} on ${CC_TERMS.length} machines`, '<a href="#" data-dgo="dTerminals">All terminals</a>')}<div class="cc-cols" style="grid-template-columns:repeat(auto-fit,minmax(260px,1fr))">${CC_TERMS.filter(m => m.name !== gone).map(tMach).join("")}</div></div>`;
  const eq = 'class="cc-cols" style="grid-template-columns:minmax(0,1fr) minmax(0,1fr)"';
  // A wide window (1280px of content or more) splits Home in two: what to act on at the left,
  // machines and what is coming at the right.
  const balanced = S.d.wide ? `${offNote}<div class="cc-wide"><div>${bNeed}${bRun}${bTerm}${bUpd}${bGit}</div><div>${bMach}${bComing}</div></div>`
    : `${offNote}${bMach}<div ${eq}>${bRun}${bNeed}</div>${bTerm}<div ${eq}>${bUpd}${bComing}</div>${bGit}`;
  const hv = S.d.home || "balanced";
  const body = !old && hv === "balanced" ? balanced : !old && hv === "calm" ? calm : old
    ? `<div class="cc-note"><span class="ddot off"></span><span>This server is older than the app, so machines and running sessions are hidden. Chats, approvals and Coming up still work.</span></div>
       <div class="cc-cols">${needSec}${comingSec}</div>${updSec}`
    : `${offNote}${machSec}<div class="cc-cols">${runSec}${needSec}</div><div class="cc-cols">${updSec}${comingSec}</div>`;
  const win = `<div class="cc-win" role="main" aria-label="Beep">${side}
    <div class="cc-main">
      <div class="cc-top"><h2>Home</h2><small>Wednesday, 4 November</small></div>
      <div class="cc-view">${body}</div>
      <div class="cc-comp"><span class="in"><span class="caret"></span><span class="ph">${empty || old ? "Ask Beep anything" : "Ask Beep, or start an agent on a machine"}</span></span>
        ${empty || old ? "" : '<button class="dbtn quiet" data-dgo="dAgentNew" title="Start a coding agent on a machine">Agent</button>'}<button class="dbtn pri" data-toast="Sent">Send<kbd>${DI.ret}</kbd></button></div>
    </div></div>`;
  return mbar() + win;
};
document.getElementById("desk").addEventListener("click", e => {
  const b = e.target.closest("[data-cc]");
  if (!b) return;
  const box = b.closest(".cc-ask");
  box.style.opacity = ".55";
  box.querySelector(".acts").innerHTML = `<span class="ok">${b.dataset.cc.slice(3)}</span>`;
});
// Before the first answer from the server: animated dots, never a blank page.
// After 15 s it says what is wrong and offers Retry; it keeps retrying anyway.
function ccConnecting(stuck, problem) {
  const skel = [70, 55, 62].map(w => `<div class="cc-skel" style="width:${w}%"></div>`).join("");
  const side = `<aside class="cc-side"><div class="cc-lights"><i></i><i></i><i></i></div><div class="cc-brand"><b>Beep</b></div>
      <div class="cc-grp"><button class="cc-si">${CC_IC.search}<span class="tt">Search</span><kbd>⌘K</kbd></button><button class="cc-si" aria-current="true">${CC_IC.home}<span class="tt">Home</span></button></div>
      <div class="cc-grp"><h3>Chats</h3>${skel}</div>
      <div class="cc-grp"><h3>Beep</h3><button class="cc-si">${CC_IC.mem}<span class="tt">Memories</span></button><button class="cc-si">${CC_IC.loop}<span class="tt">Routines</span></button><button class="cc-si">${CC_IC.gear}<span class="tt">Settings</span><kbd>⌘,</kbd></button></div>
      <div class="cc-foot"><span><span class="ddot ${problem ? problem.dot : stuck ? "bad" : "off"}"></span> ${problem ? problem.foot : stuck ? "Not connected" : "Connecting…"}</span></div></aside>`;
  const body = problem
    ? `<div class="cc-conn stuck"><span class="ddot ${problem.dot}"></span><b>${problem.title}</b>
        <span style="max-width:46ch">${problem.text}</span>
        <div style="display:flex;gap:8px">${problem.action ? `<button class="dbtn pri" data-toast="${problem.action}">${problem.action}</button>` : ""}<button class="dbtn${problem.action ? "" : " pri"}" data-toast="Trying again">Try again</button><button class="dbtn" data-toast="Opens the log">Show log</button></div></div>`
    : stuck
    ? `<div class="cc-conn"><div class="cc-pulse"><i></i><i></i><i></i></div><b>Still connecting to Beep</b>
        <span style="max-width:46ch">No answer from the server for 40 seconds. Still trying.</span>
        <div style="display:flex;gap:8px"><button class="dbtn pri" data-toast="Retrying now">Retry now</button><button class="dbtn" data-toast="Opens ~/Library/Logs/Beep">Show log</button></div></div>`
    : `<div class="cc-conn"><div class="cc-pulse"><i></i><i></i><i></i></div><b>Connecting to Beep…</b></div>`;
  return `<div class="cc-win" role="main" aria-label="Beep">${side}
    <div class="cc-main"><div class="cc-top"><h2>Home</h2><small>Wednesday, 4 November</small></div>
      <div class="cc-view" style="display:flex">${body}</div>
      <div class="cc-comp hc"><span class="in"><span class="caret"></span><span class="ph">Ask Beep anything</span></span><button class="dbtn pri" data-toast="Sends once connected">Send<kbd>${DI.ret}</kbd></button></div>
    </div></div>`;
}
VARIANTS.dHome = [["Balanced", () => { S.d.home = "balanced"; render(); }, () => (S.d.home || "balanced") === "balanced"], ["Calm", () => { S.d.home = "calm"; render(); }, () => S.d.home === "calm"], ["Detailed", () => { S.d.home = "full"; render(); }, () => S.d.home === "full"]].concat([["Default", "default"], ["Empty", "empty"], ["Node offline", "offline"], ["Old server", "old"], ["Connecting", "connecting"], ["Still connecting", "stuck"]].map(([l, k]) => [l, () => { S.d.cc = k; render(); }, () => (S.d.cc || "default") === k]));
VARIANTS.dHome.push(["Wide window", () => { S.d.wide = !S.d.wide; render(); }, () => !!S.d.wide]);
VARIANTS.dHome.push(...[["Git: projects", "default"], ["Git: host error", "error"], ["Git: no host", "none"]].map(([l, k]) => [l, () => { S.d.git = k; render(); }, () => (S.d.git || "default") === k]));
DESK_SCREENS.splice(DESK_SCREENS.findIndex(g => g.group === "Machines"), 0, { group: "Command center", items: [["dHome", "Home", "CommandCenterWindow, Home"]] });
