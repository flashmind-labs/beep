// ---- Desktop: skins, Home in tabs, the project in a split (2026-11-03) ----
{
  const st = document.createElement("style");
  st.textContent = `
.mac .cc-win.sk-flashmind { --d-ink: #080e14; --d-surface: #0d161e; --d-elev: #16242d; --d-border: #26323b; --d-fg: #f1f4f2; --d-muted: #9baab1; --d-dim: #60747d; --d-emerald: #82e7e0; --d-coral: #f09b83; --d-err: #f07878; background: var(--d-ink) }
.mac .cc-win.sk-paper { --d-ink: #fafaf9; --d-surface: #fff; --d-elev: #f0f0ee; --d-border: #e2e2de; --d-fg: #1d1f21; --d-muted: #6b6f74; --d-dim: #9a9ea3; --d-emerald: #1f5fbf; --d-coral: #c9473a; --d-err: #c9473a; background: var(--d-ink) }
.mac .cc-win .cc-side { background: var(--d-ink) }
.mac .cc-win.sk-paper :is(h2, .cc-side *) { color: var(--d-fg) }
.mac .cc-view > * { flex: none }
.mac .cc-win.sk-paper .cc-ag.codex, .mac .cc-win.sk-paper .cc-ag.codex svg { color: var(--d-fg) !important }
.mac .cc-conn.stuck .ddot { margin-bottom: 2px }
.mac .cc-top .tools { margin-left: auto; display: flex; gap: 6px; flex: none }
.mac .cc-top .tools .dbtn { white-space: nowrap }
.mac .cc-grp { grid-template-columns: minmax(0, 1fr) }
.mac .cc-si .tt { display: block; flex: 0 1 auto; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis }
.mac .cc-pal .q > span { min-width: 0; overflow: hidden; white-space: nowrap }
.mac .cc-top .us { margin-left: auto }
.mac .cc-top .tools ~ .us { margin-left: 10px }
.mac .cc-top .us ~ .nf-bell { margin-left: 10px }
.mac .us-row { gap: 10px }
.mac .us-row > :first-child { flex: 1 1 0; min-width: 60px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .us-row > .us-r { flex: 0 1 auto; max-width: 60%; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: right }
.mac .cc-si .tt + kbd { margin-left: auto }
.mac .cc-si kbd { font: inherit; font-size: 10.5px; padding: 0 4px; border-radius: 4px; border: 1px solid var(--d-border); background: var(--d-elev); color: var(--d-muted) }
.mac .dbtn.small { min-height: 22px; padding: 0 9px; font-size: 12px; gap: 4px }
.mac .cc-sec { align-items: center }
.mac .cc-sec h4 .n { margin-left: 6px; color: var(--d-muted); font-weight: 500 }
.mac .cc-sec h4 .lk { all: unset; cursor: pointer }
.mac .cc-sec h4 .lk:hover { color: var(--d-fg) }
.mac .cc-hgrid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(320px, 100%), 1fr)); gap: 14px; align-items: start }
.mac .cc-git { display: grid; gap: 14px }
.mac .cc-mstrip { display: flex; flex-wrap: wrap; border: 1px solid var(--d-border); border-radius: 8px; background: var(--d-surface); overflow: hidden }
.mac .cc-mstrip > .m { display: flex; align-items: center; gap: 8px; padding: 7px 12px; border: 0; border-right: 1px solid var(--d-border); background: none; color: var(--d-muted); font: inherit; font-size: 12px; white-space: nowrap; min-width: 0 }
.mac .cc-mstrip > button.m:hover { background: var(--d-elev) }
.mac .cc-mstrip > .m b { color: var(--d-fg); font-weight: 600 }
.mac .cc-mstrip > .m.off b { color: var(--d-muted) }
.mac .cc-mstrip > .m .bad { color: var(--d-err) }
.mac .cc-mstrip > .dbtn { margin: 4px 6px 4px auto }
.mac .cc-tabs { position: sticky; top: -14px; z-index: 2; display: flex; gap: 2px; margin: 0 -16px; padding: 0 12px; background: var(--d-ink); border-bottom: 1px solid var(--d-border); overflow-x: auto; scrollbar-width: none }
.mac .cc-tabs button { flex: none; display: flex; align-items: center; gap: 6px; padding: 9px 9px 7px; border: 0; border-bottom: 2px solid transparent; background: none; color: var(--d-muted); font: inherit; font-size: 12.5px; cursor: pointer }
.mac .cc-tabs button:hover { color: var(--d-fg) }
.mac .cc-tabs button[aria-selected="true"] { color: var(--d-fg); font-weight: 600; border-bottom-color: var(--d-emerald) }
.mac .cc-tabs .n { font-size: 11px; font-weight: 400; color: var(--d-muted) }
.mac .cc-row.bare { grid-template-columns: minmax(0, 1fr) auto }
.mac .cc-row:has(.cc-when) { grid-template-columns: 58px minmax(0, 1fr) auto }
.mac .cc-line { cursor: pointer }
.mac .cc-line:hover { background: rgba(227,228,230,.04) }
.mac .cc-win.sk-paper .cc-line:hover { background: rgba(29,31,33,.04) }
.mac .cc-line .t { all: unset; display: grid; min-width: 0; line-height: 18px; cursor: pointer }
.mac .cc-line .t b { font-weight: 500; font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-line .t > span { font-size: 11.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-line .r { grid-auto-flow: column; align-items: center }
.mac .cc-row .cc-pdot { justify-self: center }
.mac .cc-gr .r .age { min-width: 40px; text-align: right }
.mac .cc-row.cc-gr.it { padding: 5px 11px 5px 30px }
.mac .cc-gr.it .ic { display: grid; color: var(--d-muted) }
.mac .cc-gr.it .t { display: flex; gap: 6px; align-items: baseline }
.mac .cc-gr.it .t .num { flex: none; font-size: 11.5px; color: var(--d-muted) }
.mac .cc-gr.it .t b { font-weight: 450; min-width: 0 }
.mac .cc-gr.it.more { padding: 3px 11px 6px 26px }
.mac .cc-gr.it.more + .cc-row { border-top: 1px solid var(--d-border) }
.mac .cc-open > .pj-labels { padding: 8px 12px 4px; border-bottom: 0 }
.mac .pj-labels .pj-tag { margin-left: 0; font: inherit; font-size: 10px; line-height: 14px }
.mac .pj-ctl { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; padding: 7px 11px; border-bottom: 1px solid var(--d-border) }
.mac .pj-ctl .seg { display: inline-flex; gap: 2px }
.mac .pj-ctl .sp { flex: 1 }
.mac .pj-ctl .on { font-weight: 600; box-shadow: inset 0 0 0 1px var(--d-border) }
.mac .pj-list .pj-ctl { padding: 2px 14px 6px; border-bottom: 0 }
.mac .cc-open > .pj-ctl { border-bottom: 0 }
.mac .pj-prow { display: flex; align-items: center; gap: 6px }
.mac .pj-prow > label { padding-left: 10px }
.mac .pj-prow > .pj-li { flex: 1; min-width: 0 }
.mac .pj-start { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 9px 12px; border-top: 1px solid var(--d-border); font-size: 12px }
.mac .pj-start .sp { flex: 1 }
.mac .pj-start.ask { flex-direction: column; align-items: flex-start }
.mac .pj-start.ask .acts { display: flex; gap: 6px }
.mac .pj-start .skip { color: var(--d-muted) }
.mac .pj-proj { font-size: 11px; color: var(--d-muted); margin-left: 6px }
.mac .pj-tag { display: inline-block; margin-left: 4px; padding: 0 6px; font-style: normal; font-size: 10px; line-height: 14px; vertical-align: 1px; border-radius: 999px; border: 1px solid var(--d-border); background: var(--d-elev); color: var(--d-muted) }
.mac .pj-tag.lab { cursor: pointer }
.mac .pj-tag.lab:hover { color: var(--d-fg) }
.mac .pj-tag.lab.on { color: var(--d-fg); border-color: var(--d-emerald) }
.mac .cc-comp.hc { margin: 0; padding: 6px 10px 7px 16px; border: 0; border-top: 1px solid var(--d-border); border-radius: 0; background: var(--d-surface) }
.mac .nb-main { display: grid; grid-template-rows: auto auto auto minmax(0, 1fr) auto; grid-template-columns: minmax(0, 1fr); min-width: 0; min-height: 0; height: 100% }
.mac .nb-strip { display: flex; border-bottom: 1px solid var(--d-border); background: var(--d-surface); font-size: 12px; min-width: 0; overflow: hidden }
.mac .nb-strip > span { display: flex; gap: 8px; align-items: center; padding: 7px 14px; border-right: 1px solid var(--d-border); white-space: nowrap; color: var(--d-muted) }
.mac .nb-strip > span b { color: var(--d-fg); font-weight: 600 }
.mac .nb-strip > span:last-child { margin-left: auto; border-right: 0 }
.mac .nb-tabs { display: flex; gap: 2px; padding: 0 12px; border-bottom: 1px solid var(--d-border) }
.mac .nb-tabs button { font: inherit; font-size: 12.5px; background: none; border: 0; border-bottom: 2px solid transparent; color: var(--d-muted); padding: 9px 9px 7px; cursor: pointer; display: flex; gap: 6px; align-items: center }
.mac .nb-tabs button[aria-selected="true"] { color: var(--d-fg); border-bottom-color: var(--d-emerald); font-weight: 600 }
.mac .nb-tabs .n { font-size: 11px; font-weight: 400; color: var(--d-muted) }
.mac .nb-feed { padding: 14px 16px; display: grid; gap: 14px; align-content: start; overflow: auto; min-height: 0 }
.mac .nb-two { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(380px, 100%), 1fr)); gap: 14px; align-items: start }
.mac .nb-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(320px, 100%), 1fr)); gap: 14px; align-items: start }
.mac .nb-sec h4 .lk { all: unset; cursor: pointer } .mac .nb-sec h4 .lk:hover { color: var(--d-fg) }
.mac .nb-sec h4 { margin: 0 0 7px; font-size: 12px; font-weight: 600; color: var(--d-muted); display: flex; gap: 8px }
.mac .nb-sec h4 .c { font-weight: 400 }
.mac .nb-row small .nb-tag { cursor: pointer; font: inherit; font-size: 10px; line-height: 14px; padding: 0 6px; margin-left: 4px; border-radius: 999px; border: 1px solid var(--d-border); background: var(--d-elev); color: var(--d-muted) }
.mac .nb-tag.on { border-color: var(--d-emerald); color: var(--d-fg) }
.mac .nb-card { border: 1px solid var(--d-border); border-radius: 8px; background: var(--d-surface); overflow: hidden }
.mac .nb-row { display: grid; grid-template-columns: 22px minmax(0, 1fr) auto; gap: 10px; align-items: center; padding: 8px 12px; border-top: 1px solid var(--d-border); font-size: 13px }
.mac .nb-row:first-child { border-top: 0 }
.mac .nb-row .t { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .nb-row small, .mac .nb-li small { display: block; color: var(--d-muted); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .nb-row .w, .mac .nb-li .w { color: var(--d-muted); font-size: 11.5px; display: flex; gap: 6px; align-items: center }
.mac .nb-dock { display: flex; gap: 8px; align-items: center; padding: 9px 16px; border-top: 1px solid var(--d-border); background: var(--d-surface) }
.mac .nb-dock .ph { color: var(--d-dim); margin-right: auto; font-size: 13px }
.mac .nb-split { display: grid; grid-template-columns: 300px minmax(0, 1fr); min-height: 0 }
.mac .nb-list { border-right: 1px solid var(--d-border); overflow: auto; padding-bottom: 8px }
.mac .nb-lg { font-size: 12px; font-weight: 600; color: var(--d-muted); padding: 11px 14px 4px; display: flex; gap: 8px }
.mac .nb-lg .c { font-weight: 400 }
.mac .nb-li { display: grid; grid-template-columns: 26px minmax(0, 1fr) auto; gap: 8px; padding: 6px 14px; align-items: center; cursor: pointer; font-size: 13px; border: 0; background: none; color: inherit; text-align: left; width: 100% }
.mac .nb-li .t { min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .nb-li[aria-current="true"] { background: var(--d-elev); box-shadow: inset 2px 0 var(--d-emerald) }
.mac .nb-det { display: grid; grid-template-rows: auto minmax(0, 1fr); min-width: 0; min-height: 0 }
.mac .nb-dh { padding: 14px 18px 12px; border-bottom: 1px solid var(--d-border); display: grid; gap: 6px }
.mac .nb-dh .dt { font-size: 16px; font-weight: 600 }
.mac .nb-dh .meta { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; color: var(--d-muted); font-size: 12px }
.mac .nb-dh .acts { display: flex; gap: 6px; margin-top: 6px }
.mac .nb-db { padding: 14px 18px; overflow: auto; display: grid; gap: 14px; align-content: start }
.mac .nb-md { font-size: 13px; line-height: 1.55 }
.mac .nb-md p { margin: 0 0 8px } .mac .nb-md code { font-family: var(--mono); font-size: 12px; background: var(--d-elev); border-radius: 4px; padding: 0 4px }
.mac .nb-md ul { margin: 0 0 8px; padding-left: 18px }
.mac .nb-post { border: 1px solid var(--d-border); border-radius: 8px; overflow: hidden }
.mac .nb-post > div:first-child { padding: 6px 12px; background: var(--d-surface); border-bottom: 1px solid var(--d-border); font-size: 12px; color: var(--d-muted) }
.mac .nb-post > div:first-child b { color: var(--d-fg); font-weight: 600 }
.mac .nb-post .nb-md { padding: 10px 12px }
.mac .nb-tag { font-style: normal; display: inline-block; vertical-align: 1px; margin-left: 4px; padding: 0 6px; font-size: 10px; line-height: 14px; border-radius: 999px; border: 1px solid var(--d-border); background: var(--d-elev); color: var(--d-muted) }
.mac .nb-dh .meta .nb-tag { margin-left: 0 }
.mac .nb-pill { border: 1px solid var(--d-border); border-radius: 8px; padding: 0 7px; font-size: 11px; line-height: 17px; color: var(--d-muted) }
.mac .nb-pill.ok { color: #4cb782; border-color: rgba(76,183,130,.45) } .mac .nb-pill.bad { color: var(--d-err); border-color: rgba(235,87,87,.45) } .mac .nb-pill.warn { color: #f2c94c; border-color: rgba(242,201,76,.45) }
.mac .nb-kv { display: grid; grid-template-columns: 120px minmax(0, 1fr); font-size: 12.5px }
.mac .nb-kv > span { padding: 7px 12px; border-top: 1px solid var(--d-border) } .mac .nb-kv > span:nth-child(-n+2) { border-top: 0 } .mac .nb-kv > span:nth-child(odd) { color: var(--d-muted) }
.mac .nb-skins { display: grid; gap: 6px; margin-bottom: 12px } .mac .nb-skins small { color: var(--d-muted); font-size: 12px }
.mac .nb-skins > div { display: flex; flex-wrap: wrap; gap: 6px }
.mac .nb-skin { display: inline-flex; gap: 8px; align-items: center; padding: 5px 11px; border-radius: 6px; border: 1px solid var(--d-border); background: none; color: var(--d-muted); font: inherit; font-size: 13px; cursor: pointer }
.mac .nb-skin i { width: 12px; height: 12px; border-radius: 3px; border: 1px solid rgba(128,128,128,.5) }
.mac .nb-skin[aria-pressed="true"] { background: var(--d-elev); color: var(--d-fg); border-color: var(--d-emerald) }
.mac .nb-mt { display: grid; grid-template-columns: minmax(0, 1.2fr) 60px 100px 100px max-content minmax(0, 1fr) minmax(90px, .8fr) 124px; font-size: 12.5px }
.mac .nb-mt > span { display: flex; align-items: center; gap: 8px; min-width: 0; padding: 8px 12px; border-top: 1px solid var(--d-border); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .nb-mt > .th { border-top: 0; font-size: 12px; font-weight: 600; color: var(--d-muted); padding-block: 7px }
.mac .nb-mt .nm { flex-direction: column; align-items: flex-start; gap: 0 }
.mac .nb-mt .nm b { display: flex; gap: 8px; align-items: center; font-weight: 600; color: var(--d-fg) }
.mac .nb-mt small { color: var(--d-muted); font-size: 11.5px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .nb-mt .num { color: var(--d-muted); font-variant-numeric: tabular-nums; display: block; line-height: 30px }
.mac .nb-mt .num.two { display: flex; flex-direction: column; align-items: flex-start; justify-content: center; gap: 0; line-height: 16px }
.mac .nb-mt .act { justify-content: flex-end }
.mac .nb-mt .off { opacity: .55 }
.mac .nb-mt > .tr { grid-column: 1 / -1; display: grid; grid-template-columns: 10px minmax(0, 1fr) auto; gap: 10px; padding: 6px 12px 6px 34px; cursor: pointer }
.mac .nb-mt > .tr:hover { background: var(--d-elev) }
.mac .nb-mt > .tr .t { display: grid; min-width: 0 }
.mac .nb-mt > .tr b { font: 400 12px var(--mono); color: var(--d-fg) }
.mac .nb-mt > .tr .w { color: var(--d-muted); font-size: 11.5px }
@container (max-width: 700px) { .mac .nb-mt { grid-template-columns: minmax(0, 1fr) 54px 96px 80px 124px } .mac .nb-mt .c-mem, .mac .nb-mt .c-net, .mac .nb-mt .c-can { display: none } }
@container (max-width: 480px) { .mac .nb-mt { grid-template-columns: minmax(0, 1fr) auto auto } .mac .nb-mt .c-ag, .mac .nb-mt .c-disk { display: none } }
`;
  document.head.append(st);

  const skinCls = () => S.d.skin === "paper" ? " sk-paper" : S.d.skin === "flashmind" ? " sk-flashmind" : "";
  const skinned = html => html.replace('class="cc-win"', `class="cc-win${skinCls()}"`);
  const btn = (l, pri) => `<button class="dbtn${pri ? " pri" : ""}" style="padding:3px 10px;font-size:12px;min-height:24px" data-toast="${l}">${l}</button>`;
  const hc = ag => `<span class="cc-ag ${ag}" style="width:20px;height:20px;border-radius:5px;font-size:8.5px">${ag === "codex" ? "CX" : "CC"}</span>`;
  const MACH = [["build-box", "busy", "2 agents · 348 GB free", "Ubuntu 26.04 · 76 / 125 GB memory"], ["studio", "", "idle · 80 GB free", "macOS 26.3 · 24 / 36 GB memory"], ["laptop", "busy", "1 agent · 120 GB free", "macOS 27.0 · 18 / 24 GB memory"], ["phone", "off", "offline", "Android · seen 12 min ago"]];
  const machStrip = () => {
    const offline = S.d.cc === "offline";
    const tm = typeof tmRow === "function" ? tmRow() : null;
    const cells = MACH.map(([n, s, t]) => offline && n === "studio" ? [n, "off", "offline"] : [n, s, t]).map(([n, s, t]) =>
      `<button class="m${s === "off" ? " off" : ""}" data-dact="dset:htab:machines" data-dgo="dHome"><span class="ddot${s ? " " + s : ""}"></span><b>${n}</b><span>${t}</span></button>`).join("");
    const mac = tm ? `<div class="m off" title="This Mac: ${tm.title}"><span class="ddot off"></span><b>This Mac</b><span${tm.bad ? ' class="bad"' : ""}>${tm.state}</span>${S.d.tm === "off" ? `<button class="dbtn quiet small" data-dact="dset:tm:connecting" data-dgo="dHome">Add</button>` : ""}</div>` : "";
    return `<div class="cc-mstrip">${cells}${mac}<button class="dbtn quiet small" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button></div>`;
  };
  const sec = (title, count, body, right = "") => `<div class="nb-sec"><h4>${title}${count !== "" ? `<span class="c">${count}</span>` : ""}${right ? `<span class="c" style="margin-left:auto">${right}</span>` : ""}</h4><div class="nb-card">${body}</div></div>`;
  const row = (lead, t, sub, w) => `<div class="nb-row"><span>${lead}</span><span class="t">${t}<small>${sub}</small></span><span class="w">${w}</span></div>`;
  const empty = t => `<div class="nb-row" style="grid-template-columns:minmax(0,1fr);color:var(--d-dim)">${t}</div>`;

  // ---- Home: the machines strip, then tabs (Overview, PRs and issues, Projects,
  // Machines). Overview is Needs you, then boxes two by two; each box's title opens
  // its tab, and Terminals opens Machines. There is no Terminals tab. ----
  const PRS = [["acme/api", 41, "Cut the cache warm-up in half", "bad", "checks failing", "2 h", ["perf"]], ["you/site", 12, "Move the blog to the new theme", "warn", "your review", "5 h", ["ui"]],
    ["acme/api", 38, "Bump the sync crate", "ok", "approved", "1 d", ["deps"]], ["acme/web", 77, "Fix the login redirect", "", "draft", "3 d", ["bug"]]];
  const ISSUES = [["you/site", 5, "Dark mode flashes on load", "", "assigned", "1 d", ["bug", "ui"]], ["acme/web", 103, "Upload stalls past 2 GB", "", "", "4 d", ["bug"]], ["acme/api", 92, "Auto-sync the SDK docs", "", "", "32 d", ["docs", "ci"]]];
  const TERMS = [["cargo watch", "build-box", "acme/api · ~/code/api", "3 h", ""], ["tail -f server.log", "build-box", "~/logs", "2 d", ""], ["npm run dev", "studio", "you/site · ~/code/site", "exited 0", " off"]];
  const PROJECTS = [["acme", [["api", "build-box, studio · 2 agents", "1 min"], ["web", "studio", "20 min"], ["infra", "build-box", "3 h"]]],
    ["you", [["site", "laptop · 1 agent", "8 min"], ["dotfiles", "laptop, studio", "4 d"]]],
    ["oss", [["parser", "build-box", "2 w"], ["fmt-lint", "studio", "1 mo"]]]];
  // The Machines tab: name, state, OS, agents, free disk, memory in use, what it can run, now.
  const MT = [["build-box", "busy", "Ubuntu 26.04", "2", "348 GB free", "76 / 125 GB", "GPU · Docker", "working"], ["studio", "", "macOS 26.3", "–", "80 GB free", "24 / 36 GB", "Xcode · iOS simulator", "idle"],
    ["laptop", "busy", "macOS 27.0", "1", "120 GB free", "18 / 24 GB", "Xcode", "working"], ["phone", "off", "Android", "–", "–", "–", "Camera", "offline"]];
  const PCOL = ["#f2994a", "#8a9cf0", "#4cb782", "#f2c94c", "#b18ce0"];
  const HOME_TABS = [["overview", "Overview", ""], ["git", "PRs and issues", PRS.length + ISSUES.length], ["projects", "Projects", 7], ["machines", "Machines", ""]];
  const IC_PR = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="6" cy="5" r="2.2"/><circle cx="6" cy="19" r="2.2"/><circle cx="18" cy="19" r="2.2"/><path d="M6 7.2v9.6M18 16.8V10a3 3 0 00-3-3h-4M13 4.5L10.5 7 13 9.5"/></svg>';
  const IC_ISSUE = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="1.6" fill="currentColor"/></svg>';
  const num = n => `<span class="mono" style="color:var(--d-muted);font-size:11.5px">#${n}</span>`;
  const tag = (cls, t) => `<span class="cc-tag${cls ? " " + cls : ""}">${t}</span>`;
  const pdot = i => `<span class="cc-pdot" style="background:${PCOL[i % PCOL.length]}"></span>`;
  // section(): the title and count, a button on the right, then the body.
  // Shared by Home's lists and a project's: when it opened, newest or oldest, Select on issues, and Start N agents.
  window.__listCtl = (key, issues) => {
    const w = S.d[key + "w"] || "any", ord = S.d[key + "o"] || "newest", on = S.d[key + "s"];
    const set = (k, v) => `data-dact="dset:${key}${k}:${v}" data-dgo="${S.screen}"`;
    const seg = [["any", "Any time"], ["today", "Today"], ["yesterday", "Yesterday"], ["week", "This week"]].map(([v, l]) => `<button class="dbtn quiet small${w === v ? " on" : ""}" aria-pressed="${w === v}" ${set("w", v)}>${l}</button>`).join("");
    return `<div class="pj-ctl"><span class="seg">${seg}</span><span class="sp"></span><button class="dbtn quiet small" ${set("o", ord === "newest" ? "oldest" : "newest")}>${ord === "newest" ? "Newest first" : "Oldest first"}</button>${issues ? `<button class="dbtn quiet small${on ? " on" : ""}" aria-pressed="${!!on}" ${set("s", on ? "" : "on")}>Select</button>` : ""}</div>`;
  };
  // rows: [project, title, why it is left out or ""]; shown while Select is on.
  window.__listStart = (key, rows) => {
    if (!S.d[key + "s"]) return "";
    const go = S.screen, set = v => `data-dact="dset:${key}c:${v}" data-dgo="${go}"`;
    const n = rows.filter(r => !r[2]).length, label = `Start ${n} ${n === 1 ? "agent" : "agents"}`;
    if (!S.d[key + "c"]) return `<div class="pj-start"><span>${rows.length} picked</span><span class="sp"></span><button class="dbtn pri small" ${set("on")}>${label}</button></div>`;
    return `<div class="pj-start ask"><b>${label}?</b><span>Claude Code on build-box, one new worktree each, one after another. Each runs on your plan.</span>${rows.map(r => `<span class="${r[2] ? "skip" : ""}">${r[0]} ${r[1]}${r[2] ? ` (${r[2]})` : ""}</span>`).join("")}<span class="acts"><button class="dbtn pri small" data-toast="Starts ${n} agents, one at a time">${label}</button><button class="dbtn quiet small" ${set("")}>Cancel</button></span></div>`;
  };
  const csec = (title, count, body, action = "") => `<div><div class="cc-sec"><h4>${title}${count !== "" ? `<span class="n">${count}</span>` : ""}</h4>${action}</div>${body}</div>`;
  const panel = rows => `<div class="cc-panel">${rows}</div>`;
  // line(): a title and one muted line, opening on a click, with its right side.
  const ln = (lead, t, sub, right, go = "") => `<div class="cc-row cc-line${lead ? "" : " bare"}" ${go}>${lead}<button class="t" title="${t}"><b>${t}</b><span>${sub}</span></button><span class="r">${right}</span></div>`;
  const small = (l, pri) => `<button class="dbtn${pri ? " pri" : ""} small" data-toast="${l}">${l}</button>`;
  const beepAv = i => `<span class="cc-ag beep">${i}</span>`;
  const NEEDS = () => ln(hc0("claude"), "May it run xcodebuild on laptop?", "Claude Code · Swipeable approval strip on iOS", small("Answer", true), 'data-dact="dset:ags:asks" data-dgo="dAgentSession"')
    + ln(beepAv(CC_IC.chat), "Send the weekly digest to the team?", "Gmail · Inbox digest", small("Allow", true), 'data-dgo="dWinChat"');
  const hc0 = ag => `<span class="cc-ag ${ag}">${ag === "codex" ? "CX" : "CC"}</span>`;
  const RUN = [["claude", "Swipeable approval strip on iOS", "you/site · laptop", tag("wait", "needs you")], ["codex", "Fix the device build script", "acme/api · build-box", "4 min"], ["claude", "Clippy fixes in the sync crate", "acme/api · build-box", "31 min"], ["", "Will solid-state batteries beat lithium by 2030?", "Research · round 2", "2 min"]];
  const running = () => RUN.map(([ag, t, sub, right]) => ln(ag ? hc0(ag) : "", t, sub, right, ag ? 'data-dact="dset:ags:nochange" data-dgo="dAgentSession"' : 'data-dact="dset:rp:open" data-dgo="dWinResearch"')).join("");
  const coming = CC_COMING.map(c => ln(`<span class="cc-when"><b>${c[0]}</b>${c[1]}</span>`, c[2], c[3], "", 'data-dgo="dWinRoutines"')).join("");
  const X = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>';
  const UPDATES = [["Dentist moved to Thursday", "G", "Gmail · 09:41"], ["Sam shared the Q4 budget", "S", "Slack · Yesterday"]];
  const updates = () => csec("Updates", UPDATES.length, panel(UPDATES.map(([t, i, sub]) => ln(beepAv(i), t, sub, `<button class="dbtn quiet" style="padding:2px 6px" aria-label="Dismiss" data-toast="Dismissed">${X}</button>`, 'title="Ask Beep about this" data-dact="dset:wc:update" data-dgo="dWinChat"')).join("")), `<button class="dbtn quiet small" data-toast="Cleared">Clear</button>`);
  const STATE_CLS = { bad: "bad", warn: "wait", ok: "run", "": "" };
  const itemIc = kind => kind === "pr" ? IC_PR : IC_ISSUE;
  // itemLine(): an open item in an Overview box.
  const itemLine = kind => ([repo, n, t, cls, st, age]) => ln(itemIc(kind), `#${n} ${t}`, st ? `<span class="cc-sub"><i class="chip ${STATE_CLS[cls]}">${st}</i>${repo}</span>` : repo, `${age}`, `title="Opens #${n}" data-toast="Opens #${n} on its host"`);
  // gitSection(): a row per project, its items under it, Review and Reply on each.
  const gitCol = (kind, title, list, key, none) => {
    const shown = S.d[key] ? list.filter(x => x[6].includes(S.d[key])) : list;
    const all = [...new Set(list.flatMap(x => x[6]))].sort();
    const chip = (tagName, l) => `<${tagName} class="pj-tag lab${S.d[key] === l ? " on" : ""}" data-dact="dset:${key}:${S.d[key] === l ? "" : l}" data-dgo="dHome">${l}</${tagName}>`;
    const w = S.d[key + "w"] || "any", picking = kind === "issue" && S.d[key + "s"];
    const ageH = x => parseFloat(x[5]) * (x[5].endsWith("d") ? 24 : 1);
    const keep = shown.filter(x => w === "any" || (w === "today" && x[5].endsWith("h")) || (w === "yesterday" && x[5] === "1 d") || (w === "week" && (x[5].endsWith("h") || parseFloat(x[5]) <= 6 && x[5].endsWith("d")))).sort((x, y) => S.d[key + "o"] === "oldest" ? ageH(y) - ageH(x) : ageH(x) - ageH(y));
    const body = !keep.length ? `<div class="cc-empty"><span>${none}</span></div>` : keep.map(([repo, n, t, cls, st, age, labels]) => `<div class="cc-row cc-line cc-gr it" data-toast="Opens #${n} on its host">${picking ? `<label><input type="checkbox"${n === 5 || n === 92 ? " checked" : ""} aria-label="Pick #${n}"></label>` : ""}<span class="ic">${itemIc(kind)}</span><button class="t" title="#${n}"><span class="num">#${n}</span><b>${t}</b><span class="pj-proj">${repo}</span>${labels.map(l => chip("i", l)).join("")}</button><span class="r">${st ? tag(STATE_CLS[cls], st) : ""}<span class="age">${age}</span>${kind === "pr" ? small("Review") : ""}${small("Reply")}</span></div>`).join("");
    const start = kind === "issue" ? __listStart(key, [["you/site", "#5 Dark mode flashes on load", ""], ["acme/api", "#92 Auto-sync the SDK docs", ""], ["oss/parser", "#14 Handle empty input", "no machine has a checkout"]]) : "";
    return csec(title, shown.length, `<div class="cc-panel cc-open">${__listCtl(key, kind === "issue")}${all.length ? `<div class="pj-labels">${all.map(l => chip("button", l)).join("")}</div>` : ""}${body}${start}</div>`);
  };
  const term = ([t, m, where, st, off]) => ln(`<span class="ddot${off}"></span>`, t, `${m} · ${where}`, st, 'data-dgo="dTerminals"');
  const box = (title, count, go, body) => csec(`<button class="lk" ${go}>${title}</button>`, count, panel(body));
  const tabGo = k => `data-dact="dset:htab:${k}" data-dgo="dHome"`;
  const two = (a, b) => `<div class="cc-cols">${a}${b}</div>`;
  const MT_TRAFFIC = {
    "build-box": ["Link 508 MB · File sync 40 MB · Transfers 17 GB", "14 GB / 3.2 GB"],
    studio: ["Link 96 MB · File sync 2.1 GB", "1.9 GB / 310 MB"],
    laptop: ["Link 41 MB · File sync 860 MB · Transfers 1.2 GB", "1.4 GB / 700 MB"],
  };
  const MT_SYNC = { "build-box": [["acme/api", "~/code/api · 12 files left", '<span class="fs-pct"><span class="fs-bar"><i style="width:63%"></i></span>63%</span>']], studio: [["you/site", "~/code/site", '<span class="pb-chip">In sync</span>']] };
  const homeBody = tab => {
    if (tab === "git") return `<div class="cc-git" id="sec-git">${two(gitCol("pr", "Pull requests", PRS, "hlp", "No open pull requests."), gitCol("issue", "Issues", ISSUES, "hli", "No open issues."))}</div>`;
    // Machines: one full-width table, a row per machine with New terminal, its
    // terminals indented under it as sub-rows (command, project and folder, state).
    if (tab === "machines") {
      const offline = S.d.cc === "offline";
      const rows = MT.map(m => offline && m[0] === "studio" ? [m[0], "off", m[2], "–", "–", "–", m[6], "offline"] : m);
      const live = rows.filter(m => m[1] !== "off").length;
      const head = ["Machine", "Agents", "Disk", "Memory", "Sent / received", "Can run", "Now", ""].map((t, i) => `<span class="th${["", " c-ag", " c-disk", " c-mem", " c-net", " c-can", "", ""][i]}">${t}</span>`).join("");
      const body = rows.map(([n, s, os, ag, disk, mem, can, now]) => {
        const off = s === "off" ? " off" : "";
        const terms = TERMS.filter(x => x[1] === n).map(([t, , where, st, toff]) => `<span class="tr${off}" data-dgo="dTerminals"><span class="ddot${toff}"></span><span class="t"><b>${t}</b><small>${where}</small></span><span class="w">${st}</span></span>`).join("")
          + (MT_SYNC[n] || []).map(([pr, meta, right]) => `<span class="tr" data-dgo="dWinProject" title="${meta}"><span class="ddot"></span><span class="t"><b>${pr}</b><small>${meta}</small></span><span class="w">${right}</span></span>`).join("");
        return `<span class="nm${off}"><b><span class="ddot${s ? " " + s : ""}"></span>${n}</b><small>${os}</small></span><span class="num c-ag${off}">${ag}</span><span class="num c-disk${off}${n === "studio" ? " two" : ""}">${disk}${n === "studio" ? "<small>Scratch 1.2 TB free</small>" : ""}</span><span class="num c-mem${off}">${mem}</span><span class="num c-net${off}"${MT_TRAFFIC[n] ? ` title="${MT_TRAFFIC[n][0]}"` : ""}>${MT_TRAFFIC[n]?.[1] ?? "–"}</span><span class="num c-can${off}">${can}</span><span class="num${off}"${off ? ' title="Seen 12 min ago"' : ""}>${now}</span>`
          + `<span class="act">${off ? `<button class="dbtn" disabled style="padding:3px 10px;font-size:12px;min-height:24px">New terminal</button>` : `<button class="dbtn" style="padding:3px 10px;font-size:12px;min-height:24px" data-dact="dset:tl:new" data-dgo="dTerminals">New terminal</button>`}</span>${terms}`;
      }).join("");
      return csec("Machines", `${live} of ${rows.length} online`, `<div class="cc-panel"><div class="nb-mt" role="table">${head}${body}${tmCells()}${tmRow() ? "<span></span>" : ""}</div></div>`, `<button class="dbtn small" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button>`);
    }
    if (tab === "projects") {
      // A panel per owner, as many across as fit; a lone owner keeps one column's width.
      const groups = S.d.horg ? PROJECTS.slice(0, 1) : PROJECTS;
      return `<div class="cc-hgrid" id="sec-projects-home">${groups.map(([o, ps], k) => csec(o, ps.length, panel(ps.map(([n, sub, age], i) => ln(pdot(k * 3 + i), n, sub, age, 'data-dgo="dWinProject"')).join("")))).join("")}</div>`;
    }
    const recent = [["acme/api", "build-box, studio · 2 agents", "1 min"], ["you/site", "laptop · 1 agent", "8 min"], ["acme/web", "studio", "20 min"], ["acme/infra", "build-box", "3 h"]];
    if (S.d.cc === "empty") return `<div class="cc-panel cc-empty"><span>Nothing needs you, and nothing is running.</span></div>`;
    const projBox = box("Projects", 7, tabGo("projects"), recent.map(([n, sub, age], i) => ln(pdot(i), n, sub, age, 'data-dgo="dWinProject"')).join(""));
    if (S.d.cc === "old") return csec("Needs you", 1, panel(ln(beepAv(CC_IC.chat), "Send the weekly digest to the team?", "Gmail · Inbox digest", small("Allow", true), 'data-dgo="dWinChat"'))) + csec("Coming up", CC_COMING.length, panel(coming));
    return csec("Needs you", 2, panel(NEEDS())) + updates()
      + (S.d.sy === "insync" ? "" : box("Sync conflicts", 3, 'data-dgo="dSync"', fsConflict("notes (conflict 1a2b3c4d).txt", "acme/widget · Another copy of notes.txt · on build-box, studio") + fsConflict(".git/refs/beep-conflicts/heads/main-5e6f7a8b", "acme/widget · Another commit of main · on build-box") + fsConflict("docs/plan (conflict 9fbd5479).md", "acme/platform/mesh-relay · Another copy of docs/plan.md · on build-box, mac-box")))
      + two(box("Running", RUN.length, 'data-dgo="dAgents"', running()), projBox)
      + two(box("Pull requests", PRS.length, tabGo("git"), PRS.map(itemLine("pr")).join("")), box("Issues", ISSUES.length, tabGo("git"), ISSUES.map(itemLine("issue")).join("")))
      + two(box("Terminals", TERMS.length, tabGo("machines"), TERMS.map(term).join("")), box("Coming up", CC_COMING.length, 'data-dgo="dWinRoutines"', coming));
  };
  const baseHome = D.dHome;
  // States the Home view does not draw itself (connecting, stuck) keep the earlier frame.
  const ownHome = () => !["connecting", "stuck"].includes(S.d.cc);
  const PROBLEMS = {
    unlinked: { dot: "off", foot: "Not linked", title: "This Mac is not linked yet", text: "Link it to your phone to see your chats, machines and approvals here.", action: "Link to your phone" },
    revoked: { dot: "bad", foot: "Removed", title: "This Mac was removed", text: "Your phone took this computer off the account. To use Beep here again, start over with a new key and link it from the phone.", action: "Start over" },
    refused: { dot: "bad", foot: "Refused", title: "The server turned this Mac away", text: "It did not find this Mac's account.", action: null },
  };
  D.dHome = () => {
    if (S.d.cc === "startup") {
      const main = `<div class="cc-top"><h2>Home</h2><small>Wednesday, 4 November</small></div>
        <div class="cc-view"><div class="cc-mstrip"><button class="dbtn quiet small" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button></div>
        <div class="cc-tabs" role="tablist"><button role="tab" aria-selected="true">Overview</button><button role="tab" aria-selected="false" data-startup-machines>Machines</button></div>
        <div class="cc-panel cc-empty"><b>Start an agent</b><span>Your machines connect in the background.</span><button class="dbtn small" data-dgo="dAgentNew">Start an agent</button></div></div>`;
      return window.__mrmOver(skinned(ccWin(ccSide("home", { bare: true, noChats: true }), main)));
    }
    if (PROBLEMS[S.d.cc]) return mbar() + ccConnecting(false, PROBLEMS[S.d.cc]);
    if (!ownHome()) return baseHome();
    const cc = S.d.cc, old = cc === "old", empty = cc === "empty";
    const shownTabs = HOME_TABS.filter(([k]) => old ? k === "overview" : empty ? k === "overview" || k === "machines" : true);
    const tab = shownTabs.some(([k]) => k === S.d.htab) ? S.d.htab : "overview";
    const tabs = `<div class="cc-tabs" role="tablist">${shownTabs.map(([k, l, n]) => `<button role="tab" aria-selected="${k === tab}" data-dact="dset:htab:${k}" data-dgo="dHome">${l}${n !== "" ? `<span class="n">${n}</span>` : ""}</button>`).join("")}</div>`;
    const strip = old ? "" : empty ? `<div class="cc-mstrip"><button class="dbtn quiet small" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button></div>` : machStrip();
    const note = old ? `<div class="cc-note"><span class="ddot off"></span><span>Machines and running sessions aren't available for this account or server.</span></div>`
      : cc === "offline" ? `<div class="cc-note"><span class="ddot bad"></span><span><b>studio is offline.</b> Their sessions left Running now. They come back when beep-node runs there again.</span></div>` : "";
    const none = old || empty;
    const main = `<div class="cc-top"><h2>Home</h2><small>Wednesday, 4 November</small><span class="tools"></span></div>
      <div class="cc-view">${strip}${tabs}${note}${empty && tab === "machines" ? csec("Machines", "", `<div class="cc-panel cc-empty"><b>No machines linked yet</b><button class="dbtn pri" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button></div>`, `<button class="dbtn small" data-dgo="dAddMachine">${CC_IC.plus} Add a machine</button>`) : homeBody(tab)}</div>
      <div class="cc-comp hc"><span class="in"><span class="caret"></span><span class="ph">${none ? "Ask Beep anything" : "Ask Beep, or start an agent on a machine"}</span></span>${none ? "" : `<button class="dbtn" data-dgo="dAgentNew" title="Start a coding agent on a machine">Agent</button>`}<button class="dbtn pri" data-toast="Sent">Send<kbd>${DI.ret}</kbd></button></div>`;
    return window.__mrmOver(skinned(ccWin(ccSide("home", { old, bare: empty, noChats: empty }), main)));
  };
  document.getElementById("desk").addEventListener("click", e => {
    if (!e.target.closest("[data-startup-machines]")) return;
    Object.assign(S.d, { cc: "empty", htab: "machines" });
    render();
  });
  const skinVariants = [["Skin: Flashmind", () => { S.d.skin = "flashmind"; render(); }, () => S.d.skin === "flashmind"], ["Skin: Linear", () => { S.d.skin = ""; render(); }, () => !S.d.skin], ["Skin: Light", () => { S.d.skin = "paper"; render(); }, () => S.d.skin === "paper"]];
  const keepVariants = (VARIANTS.dHome || []).filter(([l]) => !["Balanced", "Calm", "Detailed", "Default", "Wide window"].includes(l) && !l.startsWith("Git:"));
  const stuckAt = keepVariants.findIndex(([l]) => l === "Still connecting");
  keepVariants.splice(stuckAt + 1, 0, ...[["Not linked", "unlinked"], ["Removed from the account", "revoked"], ["Turned away", "refused"]].map(([l, k]) => [l, () => { Object.assign(S.d, { cc: k, mrm: "", htab: "overview" }); render(); }, () => S.d.cc === k]));
  VARIANTS.dHome = [...HOME_TABS.map(([k, l]) => [`Tab: ${l}`, () => { Object.assign(S.d, { htab: k, horg: false, cc: "default", mrm: "", tm: "" }); render(); }, () => (S.d.htab || "overview") === k && !S.d.horg && ownHome() && !S.d.tm && (S.d.cc || "default") === "default"]),
    ["Startup without server", () => { Object.assign(S.d, { cc: "startup", htab: "overview", mrm: "", tm: "" }); render(); }, () => S.d.cc === "startup"],
    ["Projects, one owner", () => { Object.assign(S.d, { htab: "projects", horg: true, cc: "default", mrm: "" }); render(); }, () => S.d.htab === "projects" && !!S.d.horg],
    ["PRs filtered by a label", () => { Object.assign(S.d, { htab: "git", hlp: "perf", cc: "default", mrm: "" }); render(); }, () => S.d.htab === "git" && !!S.d.hlp],
    ...keepVariants, ...skinVariants];
  const sideItem = DESK_SCREENS.find(g => g.group === "Command center").items.find(it => it[0] === "dHome");
  if (sideItem) sideItem[1] = "Home";

  // ---- Project: everything happening on the left, the pick on the right ----
  {
    const st = document.createElement("style");
    st.textContent = `.mac .cc-hgrid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(320px, 100%), 1fr)); gap: 14px; align-items: start }
.mac .cc-pjc { flex: none; margin: 0 16px 14px; display: grid; gap: 6px }
.mac .cc-pjc { margin-inline: max(16px, calc((100% - 760px) / 2)) }
.mac .cc-pjc .box { border: 1px solid var(--d-border); border-radius: var(--radius); background: color-mix(in srgb, var(--d-elev) 92%, transparent); padding: 10px 10px 8px 12px; display: grid; gap: 8px }
.mac .cc-pjc .box:focus-within { border-color: color-mix(in srgb, var(--d-emerald) 60%, transparent) }
.mac .cc-pjc textarea { display: block; width: 100%; resize: none; min-height: 21px; max-height: 200px; line-height: 21px; overflow-y: hidden; white-space: pre-wrap; overflow-wrap: anywhere; scrollbar-width: thin }
.mac .cc-pjc input, .mac .cc-pjc textarea { border: 0; outline: 0; background: transparent; color: var(--d-fg); font: inherit; font-size: 14px; caret-color: var(--d-emerald); padding: 0; min-width: 0; user-select: text }
.mac .cc-pjc input::placeholder, .mac .cc-pjc textarea::placeholder { color: var(--d-dim) }
.mac .cc-pjc .row { display: flex; align-items: center; gap: 6px; flex-wrap: wrap }
.mac .cc-pjc .sp { flex: 1 }
.mac .cc-pjc .pill { display: inline-flex; align-items: center; gap: 6px; height: 28px; padding: 0 10px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: transparent; color: var(--d-fg); font: inherit; font-size: 12.5px; cursor: pointer; white-space: nowrap; max-width: 100% }
.mac .cc-pjc .pill:hover { background: color-mix(in srgb, var(--d-fg) 6%, transparent) }
.mac .cc-pjc .pill[disabled] { color: var(--d-dim); cursor: default; background: transparent }
.mac .cc-pjc .pill small { color: var(--d-muted); font-size: 12px }
.mac .cc-pjc .pill .cc-ag { width: 18px; height: 18px; border-radius: 5px; font-size: 8.5px; flex: none }
.mac .cc-pjc .pill svg { color: var(--d-muted); flex: none }
.mac .cc-pjc .pill .cc-ag.codex svg { color: inherit }
.mac .cc-pjc .pill[aria-expanded="true"] { background: color-mix(in srgb, var(--d-fg) 6%, transparent) }
.mac .cc-pjc .note { padding: 0 4px; font-size: 11.5px; color: var(--d-muted) }
.mac .cc-pjc .cc-err:empty { display: none }
.mac .cc-pjc input.branch { height: 28px; width: 30ch; max-width: 100%; padding: 0 8px; border: 1px solid var(--d-border); border-radius: var(--radius-sm); font: 12.5px ui-monospace, SFMono-Regular, Menlo, monospace }
.mac .cc-pjc input.branch:focus { border-color: color-mix(in srgb, var(--d-emerald) 60%, transparent) }
.mac .cc-pjc [hidden] { display: none }
.mac .cc-pjc .ag-att { padding: 0 }
.mac .cc-mt.cc-pt { grid-template-columns: minmax(140px, 1fr) minmax(0, 2fr) 60px 100px }
.mac .cc-mt.cc-pt .acts { display: flex; gap: 6px; align-items: center; justify-content: flex-end }
.mac .cc-mt.cc-pt .nm b button { padding: 0; font: inherit; color: inherit; cursor: pointer; max-width: 100%; text-align: left }
.mac .cc-mt.cc-pt .nm b button:hover { color: var(--d-emerald) }
.mac .cc-mt.cc-pt .nm .os b { font-weight: 500; color: var(--d-fg); font-size: 10.5px }
.mac .cc-mt.pj-where .nm > b { display: block; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .cc-mt.pj-where .nm.wide { grid-column: span 2 }
.mac .cc-mt.pj-where > .acts { flex-wrap: nowrap }
.mac .cc-mt.pj-where > :not(.th) { padding: 12px 14px }
.mac .cc-mt.pj-where > .th { padding-left: 14px; padding-right: 14px }
.mac .cc-mt.pj-where .nm { gap: 4px }
.mac .cc-mt.pj-where .gi { display: inline-flex; align-items: center; gap: 3px; padding: 3px 6px; font-variant-numeric: tabular-nums }
.mac .cc-mt.pj-where .gi:disabled { opacity: .45 }
.mac .cc-mt.pj-where .gi .dspin { width: 10px; height: 10px }
.mac .cc-mt.pj-where .rmx { display: inline-grid; place-items: center; padding: 3px 5px }
.mac .cc-mt.pj-where .rmx:disabled { opacity: .4 }
.mac .pv-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 3px 12px; align-items: center; padding: 8px 12px }
.mac .pv-row + .pv-row { border-top: 1px solid var(--d-border) }
.mac .pv-row .t { min-width: 0; font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pv-row .t b { font-weight: 500 }
.mac .pv-row .t code { font-size: 12px; color: var(--d-muted) }
.mac .pv-row .t .dim { color: var(--d-muted) }
.mac .pv-row .m { grid-column: 1; display: flex; flex-wrap: wrap; gap: 3px 10px; font-size: 11.5px; color: var(--d-muted); min-width: 0 }
.mac .pv-row .m:empty { display: none }
.mac .pv-row .acts { grid-row: 1 / span 2; grid-column: 2; display: flex; gap: 6px; align-items: center }
.mac .pv-row .acts .bad { font-size: 11.5px; color: #f2994a; max-width: 28ch }
.mac .pv-scroll { max-height: 520px; overflow: auto }
.mac .pj-cols { display: grid; grid-template-columns: minmax(0, 1.5fr) minmax(0, 1fr); gap: 14px; align-items: start }
.mac .pj-cols.pj-half { grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) }
.mac .ca-col > div + div { margin-top: 14px }
.mac .ca-row { cursor: pointer }
.mac .ca-row:hover, .mac .ca-row:focus-visible { background: var(--d-elev); outline: none }
.mac .ca-row.bad { cursor: default; background: none }
.mac .ca-row .t { display: flex; align-items: center; gap: 6px }
.mac .ca-row .t b { font-weight: 500; flex: none }
.mac .ca-ic.cc-ag { width: 20px; height: 20px; border-radius: var(--radius-sm); font-size: 9px; flex: none; display: inline-grid }
.mac .ca-dot { flex: none }
.mac .ca-desc { color: var(--d-muted); font-weight: 400; min-width: 0; overflow: hidden; text-overflow: ellipsis }
.mac .pj-task { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 3px 10px; padding: 8px 12px; align-items: center }
.mac .pj-task + .pj-task { border-top: 1px solid var(--d-border) }
.mac .pj-task.pj-hv { grid-template-columns: minmax(0, 1fr) auto auto; gap: 6px 12px; padding: 12px 14px }
.mac .pj-task.pj-hv > .t { grid-column: 1; grid-row: 1; font-size: 13.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .pj-task.pj-hv > .m { grid-column: 1; grid-row: 2; flex-wrap: nowrap; white-space: nowrap; overflow: hidden }
.mac .pj-task.pj-hv > .pj-st { grid-column: 2; grid-row: 1 / span 2 }
.mac .pj-task.pj-hv > .acts { grid-column: 3; grid-row: 1 / span 2; flex-wrap: nowrap; opacity: 0; transition: opacity .12s }
.mac .pj-task.pj-hv:hover > .acts, .mac .pj-task.pj-hv:focus-within > .acts, .mac .pj-task.pj-hv > .acts:has(.cc-err, :disabled) { opacity: 1 }
.mac .pj-task.pj-hv:hover { background: color-mix(in srgb, var(--d-fg) 2.5%, transparent) }
.mac .pj-task .t { font-size: 13px; font-weight: 500; min-width: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pj-task .m { grid-column: 1 / -1; display: flex; flex-wrap: wrap; gap: 4px 10px; font-size: 11.5px; color: var(--d-muted); min-width: 0 }
.mac .pj-task .m:empty { display: none }
.mac .pj-task .m a { color: #8a9cf0; text-decoration: none }
.mac .pj-task .m a:hover { text-decoration: underline }
.mac .pj-task .m code { font-family: var(--mono); font-size: 11px }
.mac .pj-task .acts { grid-column: 1 / -1; display: flex; gap: 6px; align-items: center; flex-wrap: wrap }
.mac .pj-task .acts .dbtn.quiet:first-child { margin-left: -9px }
.mac .pj-labels { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; padding: 7px 11px; border-bottom: 1px solid var(--d-border) }
.mac .pj-labels .lbl, .mac .pj-labels small { font-size: 11px; color: var(--d-muted) }
.mac .pj-st { font-size: 10.5px; font-weight: 600; padding: 0 7px; line-height: 17px; border-radius: 999px; border: 1px solid var(--d-border); color: var(--d-muted); white-space: nowrap; display: inline-flex; gap: 6px; align-items: center }
.mac .pj-st.running { color: var(--d-emerald); border-color: color-mix(in srgb, var(--d-emerald) 45%, transparent) }
.mac .pj-st.review { color: #8a9cf0; border-color: color-mix(in srgb, #8a9cf0 45%, transparent) }
.mac .pj-st.blocked { color: #f2c94c; border-color: color-mix(in srgb, #f2c94c 45%, transparent) }
.mac .pj-st.done { color: var(--d-dim) }
.mac .pj-st.dropped { color: var(--d-dim); text-decoration: line-through }
.mac .pj-st .dspin { width: 9px; height: 9px }
.mac .pj-tree { padding: 6px 0 }
.mac .pj-node { display: grid; grid-template-columns: 28px minmax(0, 1fr); gap: 2px 9px; padding: 6px 10px; align-items: start }
.mac .pj-node.child { margin-left: 18px; padding-left: 14px; border-left: 1px solid var(--d-border) }
.mac .pj-node.over { opacity: .7 }
.mac .pj-node.link { cursor: pointer; border-radius: var(--radius-sm) }
.mac .pj-node.link:hover .t b { color: #8a9cf0 }
.mac .pj-node .t { display: grid; line-height: 17px; min-width: 0 }
.mac .pj-node .t b { font-size: 12.5px; font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pj-node .t span { font-size: 11.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pj-node .acts { grid-column: 2; display: flex; gap: 6px; align-items: center }
.mac .pj-node .acts .dbtn { margin-left: -9px }
.mac .pj-node .out { grid-column: 2; font-family: var(--mono); font-size: 11px; color: color-mix(in srgb, var(--d-fg) 75%, transparent); background: var(--d-ink); border: 1px solid var(--d-border); border-radius: var(--radius-sm); padding: 4px 7px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pj-plan { display: grid; gap: 6px; padding: 10px; border-radius: var(--radius); border: 1px solid var(--d-border); background: var(--d-elev) }
.mac .pj-plan b { font-size: 13px; font-weight: 600 }
.mac .pj-plan > small { font-size: 11px; color: var(--d-muted) }
.mac .pj-plan ol { margin: 0; padding-left: 18px; display: grid; gap: 3px; font-size: 12.5px }
.mac .pj-plan ol small { color: var(--d-muted) }
.mac .pj-plan .acts { display: flex; gap: 6px; align-items: center }
.mac .pj-plan .ok { font-size: 12px; color: var(--d-emerald) }
.mac .sw-age { font-size: 11px; color: var(--d-muted); white-space: nowrap }
.mac .cc-sec h4 .sw-spin, .mac .cc-sec h4 .sw-age { margin-left: 8px; letter-spacing: 0; text-transform: none; font-weight: 400 }
.mac .pj-task.found { background: color-mix(in srgb, #8a9cf0 8%, transparent) }
.mac .pj-needs { display: grid; gap: 8px }
.mac .cc-view.split { padding: 0; overflow: hidden; display: block }
.mac .pj-split { display: grid; grid-template-columns: minmax(240px, 30%) minmax(0, 1fr); height: 100%; min-height: 0 }
.mac .pj-list { overflow: auto; border-right: 1px solid var(--d-border); padding-bottom: 10px; scrollbar-width: thin }
.mac .pj-list h4 { margin: 0; padding: 12px 14px 4px; font-size: 12px; font-weight: 600; letter-spacing: 0; text-transform: none; color: var(--d-dim); display: flex; gap: 8px }
.mac .pj-list h4 .n { font-weight: 400 }
.mac .pj-list .note { padding: 4px 14px 6px; font-size: 12px; color: var(--d-muted) }
.mac .pj-li { display: grid; grid-template-columns: 28px minmax(0, 1fr) auto; gap: 8px; align-items: center; width: 100%; padding: 6px 12px 6px 14px; border: 0; background: none; color: var(--d-fg); font: inherit; text-align: left; cursor: pointer }
.mac .pj-li:hover { background: color-mix(in srgb, var(--d-fg) 4%, transparent) }
.mac .pj-li[aria-current="true"] { background: var(--d-elev); box-shadow: inset 2px 0 var(--d-emerald) }
.mac .pj-li .lead { display: grid; place-items: center }
.mac .pj-li .lead svg { width: 18px; height: 18px }
.mac .pj-li .num { font-family: var(--mono); font-size: 11px; color: var(--d-muted) }
.mac .pj-li .t { display: grid; min-width: 0; line-height: 17px }
.mac .pj-li .t b { font-weight: 500; font-size: 13px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pj-li .t span { font-size: 11.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .pj-tag { display: inline-block; margin-left: 4px; padding: 0 6px; font-style: normal; font-size: 10px; line-height: 14px; vertical-align: 1px; border-radius: 999px; border: 1px solid var(--d-border); background: var(--d-elev); color: var(--d-muted) }
.mac .pj-dh .meta .pj-tag { margin-left: 0 }
.mac .pj-labels { display: flex; flex-wrap: wrap; gap: 4px; padding: 2px 14px 6px }
.mac .pj-labels .pj-tag { margin-left: 0; font: inherit; font-size: 10px; line-height: 14px }
.mac .pj-tag.lab { cursor: pointer }
.mac .pj-tag.lab:hover { color: var(--d-fg) }
.mac .pj-tag.lab.on { color: var(--d-fg); border-color: var(--d-emerald) }
.mac .cc-open > .pj-labels { padding: 8px 12px 4px }
.mac .pj-li .r { font-size: 11.5px; color: var(--d-muted); white-space: nowrap }
.mac .cc-running { display: inline-flex; align-items: center; gap: 8px }
.mac .cc-running .dspin { width: 10px; height: 10px; flex: none }
.mac .pj-det { overflow: auto; padding: 14px 18px 18px; display: flex; flex-direction: column; gap: 14px; min-width: 0; scrollbar-width: thin }
.mac .pj-det > * { flex: none }
.mac .pj-dh { display: grid; gap: 6px; padding-bottom: 12px; border-bottom: 1px solid var(--d-border) }
.mac .pj-dh h3 { margin: 0; font-size: 16px; font-weight: 600; line-height: 1.3 }
.mac .pj-dh .meta { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; font-size: 12px; color: var(--d-muted) }
.mac .pj-dh .meta b { color: var(--d-fg); font-weight: 600 }
.mac .pj-dh .acts { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin-top: 4px }
.mac .pj-dh .acts .acts { display: contents }
.mac .pj-db { display: grid; gap: 12px }
.mac .pj-md { font-size: 13px; line-height: 1.55; overflow-wrap: anywhere; min-width: 0 }
.mac .pj-md > :first-child { margin-top: 0 }
.mac .pj-md > :last-child { margin-bottom: 0 }
.mac .pj-md p, .mac .pj-md ul, .mac .pj-md ol, .mac .pj-md pre, .mac .pj-md blockquote, .mac .pj-md table { margin: 0 0 8px }
.mac .pj-md ul, .mac .pj-md ol { padding-left: 20px }
.mac .pj-md h1, .mac .pj-md h2, .mac .pj-md h3, .mac .pj-md h4 { font-size: 13.5px; margin: 12px 0 6px }
.mac .pj-md a { color: #8a9cf0 }
.mac .pj-md code { font-family: var(--mono); font-size: 12px; background: var(--d-elev); border-radius: 4px; padding: 0 4px }
.mac .pj-md pre { background: color-mix(in srgb, var(--d-ink) 80%, #000); border: 1px solid var(--d-border); border-radius: var(--radius-sm); padding: 8px 10px; overflow: auto }
.mac .pj-md pre code { background: none; padding: 0 }
.mac .pj-md blockquote { border-left: 2px solid var(--d-border-hi); padding-left: 10px; color: var(--d-muted) }
.mac .pj-md table { border-collapse: collapse }
.mac .pj-md td, .mac .pj-md th { border: 1px solid var(--d-border); padding: 3px 7px }
.mac .pj-post { border: 1px solid var(--d-border); border-radius: var(--radius); overflow: hidden }
.mac .pj-post .by { padding: 6px 12px; background: var(--d-surface); border-bottom: 1px solid var(--d-border); font-size: 12px; color: var(--d-muted) }
.mac .pj-post .by b { color: var(--d-fg); font-weight: 600 }
.mac .pj-post .pj-md { padding: 10px 12px }
.mac .pj-cbox { display: grid; gap: 8px; justify-items: end }
.mac .pj-cbox textarea { width: 100%; box-sizing: border-box; min-height: 72px; resize: none; padding: 10px 12px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: var(--d-surface); color: var(--d-fg); font: inherit; font-size: 13px; line-height: 1.5; outline: 0 }
.mac .pj-cbox textarea::placeholder { color: var(--d-dim) }
.mac .pj-cbox textarea:focus { border-color: color-mix(in srgb, var(--d-emerald) 60%, transparent) }
.mac .pj-org { display: grid; gap: 8px; margin-bottom: 14px }
.mac .pj-org h3 { margin: 0; display: flex; align-items: baseline; gap: 8px; font-size: 13px; font-weight: 600; color: var(--d-fg) }
.mac .pj-org h3 small { font-size: 11px; font-weight: 400; color: var(--d-muted) }
.mac .pj-org h3.gp { align-items: center }
.mac .pj-org h3.gp svg { color: var(--d-dim); flex: none }
.mac .pj-org h3 .gp-paths { font-weight: 400; font-size: 11px; color: var(--d-dim); font-family: var(--mono); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .pj-org h3 .gp-acts { margin-left: auto; display: inline-flex; gap: 6px; flex: none }
.mac .pj-org h3 .gp-acts .dbtn { min-height: 22px; padding: 0 9px; font-size: 12px; gap: 4px }
.mac .pj-org h3 input { padding: 1px 8px; border: 1px solid var(--d-emerald); border-radius: var(--radius-sm); background: color-mix(in srgb, var(--d-ink) 80%, #000); color: var(--d-fg); font: inherit; outline: 0; min-width: 160px }
.mac .pj-org .gp-none { font-size: 12px; color: var(--d-muted) }
.mac .cc-org.gp { display: flex; align-items: center; gap: 6px }
.mac .cc-org.gp svg { color: var(--d-dim) }
.mac .gp-path { display: flex; gap: 8px; align-items: center; padding: 6px 0; font-size: 12.5px; font-family: var(--mono); border-top: 1px solid var(--d-border) }
.mac .gp-path span { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .gp-path .dbtn { padding: 1px 8px; font-size: 11.5px; min-height: 0; font-family: inherit }
.mac .gp-field { flex: 1; min-width: 0; padding: 8px 12px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: color-mix(in srgb, var(--d-ink) 80%, #000); color: var(--d-fg); font-family: var(--mono); font-size: 12.5px; outline: 0; caret-color: var(--d-emerald); user-select: text }
.mac .gp-field:focus { border-color: var(--d-emerald) }
.mac .gp-field::placeholder { color: var(--d-dim) }
.mac .gp-foot { display: flex; gap: 8px; justify-content: flex-end; margin-top: 14px }
.mac .pj-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); gap: 10px }
.mac .pj-grid > * { min-width: 0 }
.mac .pj-card { display: grid; gap: 8px; padding: 12px 14px; border-radius: var(--radius); border: 1px solid var(--d-border); background: color-mix(in srgb, var(--d-fg) 3%, transparent); text-align: left; font-size: 12.5px; color: var(--d-fg); cursor: default }
.mac .pj-card:hover { border-color: color-mix(in srgb, var(--d-fg) 18%, transparent) }
.mac .pj-card:focus-visible { outline: 2px solid var(--d-emerald); outline-offset: 2px }
.mac .pj-card .h { display: flex; align-items: center; gap: 8px }
.mac .pj-card .h b { min-width: 0; flex: 0 1 auto; font-size: 14.5px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .pj-card .h > :not(b) { flex: none }
.mac .pj-card .h small { margin-left: auto; font-size: 11px; color: var(--d-muted); white-space: nowrap }
.mac .pj-card .pa { display: grid; padding: 2px; border: 0; background: transparent; color: var(--d-dim) }
.mac .pj-card .pa:hover, .mac .pj-card .pa.on { color: #f2994a }
.mac .pj-card .pa svg { width: 13px; height: 13px }
.mac .pj-card .line { color: var(--d-muted); line-height: 17px; overflow-wrap: anywhere }
.mac .pj-card .line b { color: var(--d-fg); font-weight: 600 }
.mac .pj-bar { display: flex; height: 5px; border-radius: 3px; overflow: hidden; background: color-mix(in srgb, var(--d-fg) 8%, transparent) }
.mac .pj-bar i { display: block; height: 100% }
.mac .pj-bar i.running { background: var(--d-emerald) }
.mac .pj-bar i.review { background: #8a9cf0 }
.mac .pj-bar i.blocked { background: #f2c94c }
.mac .pj-bar i.planned { background: var(--d-dim) }
.mac .pj-bar i.done { background: var(--d-border) }
.mac .pj-add { grid-template-columns: minmax(0, 1fr) auto; margin-bottom: 14px }
.mac .in-tabs { display: flex; gap: 6px; flex-wrap: wrap }
.mac .in-tabs .tm-tab { padding: 5px 11px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: transparent; color: var(--d-muted); font: inherit; font-size: 12.5px }
.mac .in-tabs .tm-tab[aria-current="true"] { background: color-mix(in srgb, var(--d-fg) 8%, transparent); color: var(--d-fg) }
.mac .tm-tabs .tm-tab { display: inline-flex; align-items: center; gap: 6px }
.mac .fs-tools { display: flex; align-items: center; gap: 10px }
.mac .fs-list { display: grid; margin-bottom: 14px; overflow: hidden }
.mac .fs-row { display: grid; gap: 6px; padding: 10px 12px; font-size: 12.5px }
.mac .fs-row + .fs-row, .mac .fs-conflict + .fs-conflict, .mac .fs-wait + .fs-wait { border-top: 1px solid var(--d-border) }
.mac .fs-h { display: flex; align-items: center; gap: 8px }
.mac .fs-h b { font-weight: 600 }
.mac .fs-h .pb-chip { margin-left: auto }
.mac .fs-bar { height: 3px; border-radius: 2px; background: color-mix(in srgb, var(--d-fg) 8%, transparent); overflow: hidden }
.mac .fs-bar i { display: block; height: 100%; background: color-mix(in srgb, #4cb782 60%, transparent) }
.mac .fs-pct { display: flex; align-items: center; gap: 8px }
.mac .fs-pct .fs-bar { display: block; width: 120px }
.mac .fs-m { display: flex; flex-wrap: wrap; gap: 4px 10px; color: var(--d-muted); font-size: 11.5px }
.mac .fs-m code { font-family: var(--mono); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; max-width: 100% }
.mac .fs-conflict { display: flex; align-items: center; gap: 10px; padding: 8px 12px; font-size: 12.5px }
.mac .fs-conflict .t { flex: 1; min-width: 0; display: grid; gap: 2px }
.mac .fs-conflict .t code { font-family: var(--mono); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .fs-conflict .t span { color: var(--d-muted); font-size: 11.5px }
.mac .fs-conflict .acts { display: flex; gap: 6px; flex: none }
.mac .fs-conflict.open { flex-wrap: wrap } .mac .fs-cmp { flex: 0 0 100%; border: 1px solid var(--d-border); border-radius: 8px; overflow: hidden } .mac .fs-cmph { display: flex; gap: 12px; padding: 6px 10px; font-size: 11.5px; color: var(--d-muted); border-bottom: 1px solid var(--d-border) } .mac .fs-cmph small { margin-left: auto }
.mac .fs-wait { display: flex; justify-content: space-between; gap: 10px; padding: 6px 12px; font-size: 12px }
.mac .fs-wait code { font-family: var(--mono); font-size: 11.5px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .fs-note { font-size: 12px }
.mac .rv-shard .pj-st { max-width: 180px; overflow: hidden; text-overflow: ellipsis }
.mac .pj-st.done { color: var(--d-emerald); border-color: color-mix(in srgb, var(--d-emerald) 45%, transparent) }
.mac .pj-st.bad { color: var(--d-err); border-color: color-mix(in srgb, var(--d-err) 50%, transparent) }
.mac .pj-task .acts.rv-acts .dbtn { margin-left: 0 }
.mac .pj-task.ask { background: color-mix(in srgb, #f2c94c 5%, transparent) }
.mac .pj-task .t a { color: #8a9cf0; text-decoration: none; font-weight: 600 }
.mac .cc-sec small.sw-age { color: var(--d-dim); margin-left: auto }
.mac .pj-actions { min-width: 0 }
.mac .pj-runbox {  }
.mac .pj-runrow { border-bottom: 1px solid var(--d-border) }
.mac .pj-runrow:last-child { border-bottom: 0 }
.mac .pj-runrow .rr-head { display: grid; grid-template-columns: 20px minmax(0, 1fr) auto 14px; align-items: center; gap: 12px; padding: 12px 14px; cursor: pointer; border-radius: var(--radius-sm) }
.mac .pj-runrow .rr-head:hover, .mac .pj-runrow.open .rr-head { background: color-mix(in srgb, var(--d-fg) 3%, transparent) }
.mac .pj-runrow .rr-head:focus-visible { outline: 1px solid var(--accent, #4cb782); outline-offset: -1px }
.mac .rr-st { width: 20px; height: 20px; border-radius: 50%; display: grid; place-items: center; color: var(--d-muted); background: color-mix(in srgb, var(--d-fg) 5%, transparent) }
.mac .rr-st.done { color: #4cb782; background: color-mix(in srgb, #4cb782 12%, transparent) }
.mac .rr-st.blocked { color: var(--d-err); background: color-mix(in srgb, var(--d-err) 13%, transparent) }
.mac .rr-st.running { color: #4cb782 }
.mac .rr-st .dspin { width: 11px; height: 11px }
.mac .rr-body { min-width: 0; display: grid; gap: 4px }
.mac .rr-t { display: flex; gap: 8px; align-items: baseline; min-width: 0; font-size: 13.5px }
.mac .rr-t b { flex: none; font-weight: 600; color: #8a9cf0 }
.mac .rr-t > span:last-child { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .rr-m { display: flex; gap: 10px; min-width: 0; font-size: 12px; color: var(--d-muted); white-space: nowrap; overflow: hidden }
.mac .rr-m > * { min-width: 0; overflow: hidden; text-overflow: ellipsis }
.mac .rr-m code.br { flex: 0 1 auto; max-width: 45%; font-size: 11.5px }
.mac .rr-side { display: grid; justify-items: end; gap: 4px; font-size: 12px; color: var(--d-muted); white-space: nowrap }
.mac .rr-side .rr-took { font-size: 11.5px; opacity: .75; font-variant-numeric: tabular-nums }
.mac .pj-runrow.blocked .rr-when { color: var(--d-err) }
.mac .rr-side, .mac .rr-acts { grid-column: 3; grid-row: 1; justify-self: end; transition: opacity .12s }
.mac .rr-acts { opacity: 0 }
.mac .pj-runrow .rr-head:hover .rr-acts, .mac .pj-runrow .rr-head:focus-visible .rr-acts, .mac .rr-acts:focus-within { opacity: 1 }
.mac .pj-runrow .rr-head:hover .rr-side, .mac .pj-runrow .rr-head:focus-visible .rr-side, .mac .rr-head:has(.rr-acts:focus-within) .rr-side { opacity: 0 }
.mac .rr-chev { width: 7px; height: 7px; border-right: 1.5px solid var(--d-muted); border-bottom: 1.5px solid var(--d-muted); transform: rotate(-45deg); transition: transform .15s }
.mac .rr-chev.on { transform: rotate(45deg) }
.mac .pj-runrow .pj-run { margin: 10px 14px 14px 46px; padding: 10px 12px }
.mac .pj-chk.bad, .mac .pj-task .m a.pj-chk.bad { color: #f2c94c }
.mac .pj-host { display: inline-block; font-size: 10.5px; line-height: 15px; padding: 0 5px; margin-right: 6px; border: 1px solid var(--d-border); border-radius: 4px; color: var(--d-muted); vertical-align: 1px; font-weight: 500 }
.mac .cc-sec small.hosterr { color: #f2c94c }
.mac .pj-run { grid-column: 1 / -1; display: grid; gap: 8px; margin-top: 6px; padding: 9px 10px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: var(--d-elev) }
.mac .pj-jobs { display: grid; grid-template-columns: auto 1fr; gap: 6px 14px; font-size: 12px; align-items: baseline }
.mac .pj-jobs b { font-weight: 500 }
.mac .pj-jobs b.bad { color: #f2c94c }
.mac .pj-jobs small { color: var(--d-muted) }
.mac .pj-steps { display: grid; gap: 2px; font-size: 12px }
.mac .pj-step { display: grid; grid-template-columns: 10px 1fr auto; gap: 8px; align-items: center }
.mac .pj-step small { color: var(--d-muted) }
.mac .pj-step.bad { color: #f2c94c }
.mac .pj-step.skip { color: var(--d-dim) }
.mac .pj-log small { color: var(--d-muted); font-size: 11px }
.mac .pj-log pre { margin: 4px 0 0; padding: 8px; border-radius: var(--radius-sm); background: color-mix(in srgb, #000 25%, transparent); font-family: var(--mono); font-size: 11.5px; line-height: 16px; white-space: pre-wrap; overflow-wrap: anywhere; max-height: 220px; overflow: auto; user-select: text }
.mac .pj-run .acts { display: flex; gap: 6px }
.mac .pj-task.ca-row > .t, .mac .pj-task.ca-row > .m { grid-column: 1 }
.mac .pj-task.ca-row > .acts { grid-column: 2; grid-row: 1 / span 2; align-self: center; padding-left: 12px }
.mac .pj-post.nf-on { border-color: var(--d-emerald); box-shadow: inset 2px 0 var(--d-emerald); padding-left: 10px }
.mac .pj-post.nf-on .by { display: flex; gap: 6px; align-items: center }
.mac .pj-post.nf-on .by .dbtn { margin-left: auto }
.mac .pj-list .pj-labels { border-bottom: 0; padding: 2px 14px 6px }
.mac .pj-det .pj-labels { border-bottom: 0; padding: 0 }
.mac .pj-rows { margin: 0 -18px }
.mac .pj-back { align-self: flex-start }
.mac .pj-det .cc-sec:not(:first-child) { margin-top: 18px }
.mac .pj-det .cc-sec { margin-bottom: 9px }
.mac .pj-det .cc-sec > h4 + small { margin-right: auto }
.mac .pj-det .pj-labels { gap: 6px; margin-bottom: 12px }
.mac .pj-det .pj-labels .pj-tag { font-size: 11.5px; line-height: 18px; padding: 2px 10px; border-radius: var(--radius-sm) }
.mac .pj-tag.lab.on { background: color-mix(in srgb, var(--d-emerald) 16%, transparent) }
.mac .pj-br.pj-task { gap: 2px 16px; padding: 9px 14px }
.mac .pj-br .t { display: flex; align-items: baseline; gap: 8px; grid-column: 1 }
.mac .pj-br .t code { overflow: hidden; text-overflow: ellipsis }
.mac .pj-br .m { grid-column: 1; flex-wrap: nowrap; font-size: 12px }
.mac .pj-br .m > span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .pj-br .acts { grid-column: 2; grid-row: 1 / span 2; align-self: center; flex-wrap: nowrap; margin: 0 }
.mac .br-tag { flex: none; font-size: 11.5px; font-weight: 400; color: var(--d-muted) }
.mac .br-add { flex: none; font-size: 11.5px; font-weight: 500; color: var(--d-ok); font-variant-numeric: tabular-nums }
.mac .pj-ov { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 18px; align-items: start }
.mac .pj-ov > div { display: grid; gap: 18px; min-width: 0 }
.mac .pj-ov .cc-sec:not(:first-child) { margin-top: 0 }
.mac .pj-ov .r { display: flex; gap: 4px; align-items: center }
.mac .pj-ov .r .pj-tag { margin-left: 0 }
.mac .pj-scroll { max-height: 62vh; overflow: auto; scrollbar-width: thin }
@media (max-width: 1000px) { .mac .pj-ov { grid-template-columns: minmax(0, 1fr) } }
.mac .cc-sec h4 > small { margin-left: 8px; font-weight: 400 }
.mac .pj-dh h3, .mac .pj-md, .mac .pj-post { color: var(--d-fg) }
.mac .cc-win.sk-paper .cc-ag.codex { background-color: #1d1f21 }
.mac .dv-sheet .top .dv-btn { color: var(--d-fg); border-color: var(--d-border) }
.mac .cc-note.rv-cfl { display: flex; align-items: flex-start; gap: 10px }
.mac .cc-note.rv-cfl > .ddot { margin-top: 6px; flex: none }
.mac .rv-cfl-t { display: grid; gap: 2px }
.mac .cc-line > .num { font-family: var(--mono); font-size: 11px; color: var(--d-muted); justify-self: center }
.mac .cc-line:has(> .num) { grid-template-columns: minmax(28px, max-content) minmax(0, 1fr) auto }
.mac .rv-head { display: flex; align-items: center; gap: 10px; margin-bottom: 10px }
.mac .rv-head h3 { margin: 0; font-family: var(--display); font-weight: 800; font-size: 18px; letter-spacing: -0.02em; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .rv-pill { display: inline-flex; align-items: center; gap: 6px; font-size: 11.5px; font-weight: 600; padding: 0 9px; line-height: 21px; border-radius: 999px; border: 1px solid; white-space: nowrap }
.mac .rv-pill.changes { color: #f2c94c; border-color: color-mix(in srgb, #f2c94c 50%, transparent); background: color-mix(in srgb, #f2c94c 8%, transparent) }
.mac .rv-pill.ready { color: var(--d-emerald); border-color: color-mix(in srgb, var(--d-emerald) 55%, transparent); background: color-mix(in srgb, var(--d-emerald) 10%, transparent) }
.mac .rv-pill.blocked { color: var(--d-err); border-color: color-mix(in srgb, var(--d-err) 50%, transparent); background: color-mix(in srgb, var(--d-err) 8%, transparent) }
.mac .rv-pill.running { color: var(--d-muted); border-color: var(--d-border) }
.mac .rv-pill .dspin, .mac .rv-fact .dspin, .mac .rv-ver .dspin { width: 9px; height: 9px }
.mac .rv-facts { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin-bottom: 12px }
.mac .rv-fact { display: inline-flex; align-items: center; gap: 6px; padding: 3px 10px; border-radius: 999px; border: 1px solid var(--d-border); background: var(--d-elev); font-size: 12px; white-space: nowrap }
.mac .rv-fact b { font-weight: 600 }
.mac .rv-fact code { font-family: var(--mono); font-size: 11.5px }
.mac .rv-fact.bad { border-color: color-mix(in srgb, var(--d-err) 50%, transparent); color: var(--d-err) }
.mac .rv-fact .ddot.bad { background: var(--d-err) }
.mac .rv-tools { margin-left: auto; display: flex; gap: 6px }
.mac .rv-cfl { align-items: flex-start; margin-bottom: 12px }
.mac .rv-cfl .ddot { margin-top: 6px; background: var(--d-err) }
.mac .rv-cfl-t { display: grid; gap: 2px }
.mac .rv-cfl-t span { color: var(--d-muted) }
.mac .rv-cfl-t code { font-family: var(--mono); font-size: 11.5px }
.mac .rv-find { display: grid; grid-template-columns: 70px minmax(0, 1fr) auto; gap: 3px 10px; padding: 10px 12px; align-items: start }
.mac .rv-find + .rv-find { border-top: 1px solid var(--d-border) }
.mac .rv-find.gone { opacity: .7 }
.mac .rv-sev { justify-self: start; font-size: 10.5px; font-weight: 600; line-height: 17px; padding: 0 7px; border-radius: 999px; border: 1px solid var(--d-border); color: var(--d-muted) }
.mac .rv-sev.high { color: var(--d-err); border-color: color-mix(in srgb, var(--d-err) 50%, transparent) }
.mac .rv-sev.medium { color: #f2c94c; border-color: color-mix(in srgb, #f2c94c 45%, transparent) }
.mac .rv-find .t { display: grid; gap: 2px; min-width: 0 }
.mac .rv-find .t b { font-size: 13px; font-weight: 500; line-height: 18px }
.mac .rv-find .t code { font-family: var(--mono); font-size: 11.5px; color: #8a9cf0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .rv-find .t span { font-size: 11.5px; color: var(--d-muted) }
.mac .rv-side { display: grid; justify-items: end; gap: 4px }
.mac .rv-side .dbtn { min-height: 22px; padding: 0 7px; font-size: 11.5px }
.mac .rv-ver { display: inline-flex; align-items: center; gap: 4px; font-size: 11px; color: var(--d-emerald); white-space: nowrap }
.mac .rv-ver.wait { color: var(--d-muted) }
.mac .rv-more { padding: 14px; border-top: 1px solid var(--d-border) }
.mac .rv-sum { max-width: 50ch }
.mac .rv-sum-row { padding: 8px 12px; border-top: 1px solid var(--d-border); font-size: 11.5px; color: var(--d-muted) }
.mac .rv-shard { display: grid; grid-template-columns: 28px minmax(0, 1fr) auto; gap: 2px 10px; padding: 8px 12px; align-items: center }
.mac .rv-shard + .rv-shard { border-top: 1px solid var(--d-border) }
.mac .rv-shard .t { display: grid; line-height: 17px; min-width: 0 }
.mac .rv-shard .t b { font-size: 12.5px; font-weight: 500; font-family: var(--mono) }
.mac .rv-shard .t span { font-size: 11.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .rv-shard .pj-st { max-width: 180px; overflow: hidden; text-overflow: ellipsis }
.mac .pj-task .acts.rv-acts .dbtn { margin-left: 0 }
.mac .rv-bar { position: sticky; bottom: 0; display: flex; align-items: center; gap: 8px; flex-wrap: wrap; margin-top: 14px; padding: 10px 0 12px; border-top: 1px solid var(--d-border); background: var(--d-ink) }
.mac .rv-bar small { flex-basis: 100%; font-size: 12px; color: var(--d-muted) }
.mac .dv-sheet { position: absolute; inset: 0; z-index: 60; display: grid; padding: 18px; background: color-mix(in srgb, #000 62%, transparent) }
.mac .dv-sheet > .box { min-height: 0; min-width: 0; display: flex; flex-direction: column; border: 1px solid var(--d-border); border-radius: var(--radius); background: var(--d-surface); box-shadow: 0 18px 50px color-mix(in srgb, #000 50%, transparent); overflow: hidden }
.mac .dv-sheet .top { flex: none; display: flex; align-items: center; gap: 10px; padding: 9px 10px 9px 16px; border-bottom: 1px solid var(--d-border) }
.mac .dv-sheet .top h3 { margin: 0; font: 800 17px var(--display); letter-spacing: -0.02em; white-space: nowrap }
.mac .dv-sheet .top small { min-width: 0; color: var(--d-muted); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .dv-sheet .dv { flex: 1; height: auto }`;
    document.head.append(st);
  }
  const IC_CHAT = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 5h16v11H9l-5 4z"/></svg>';
  const pb = (l, cls = "") => `<button class="dbtn${cls ? " " + cls : ""}" data-toast="${l}">${l}</button>`;
  // Pull and Push under "Where it lives": a button with nothing to do is disabled, a running one says so.
  const gb = (l, cls, off) => `<button class="dbtn ${cls}"${off ? " disabled" : ` data-toast="${l}"`}>${l}</button>`;
  // Pull and Push as icons, as VS Code's status bar: a down arrow with the commits behind, an up
  // arrow with those ahead. The tooltip says what a click moves; nothing to move dims it; a running
  // one turns its arrow into a spinner.
  const ARROW = { pull: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 5v14M6 13l6 6 6-6"/></svg>', push: '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M6 11l6-6 6 6"/></svg>' };
  const gtip = (dir, n) => (dir === "pull" ? "Pull" : "Push") + (n ? ` ${n} commit${n === 1 ? "" : "s"}` : "");
  const gi = (dir, n, off, busy) => `<button class="dbtn quiet small gi" title="${busy ? (dir === "pull" ? "Pulling…" : "Pushing…") : gtip(dir, n)}" aria-label="${gtip(dir, n)}"${off || busy ? " disabled" : ` data-toast="${gtip(dir, n)}"`}>${busy ? '<span class="dspin"></span>' : ARROW[dir]}${n}</button>`;
  const gpair = (behind, ahead, pull, push, busy = "") => gi("pull", behind, !pull, busy === "pull") + gi("push", ahead, !push, busy === "push");
  // The branch only: the counts are on the icons. "not pushed" stays, as no icon says it.
  const gnote = (branch, text, err) => `<span class="gs" style="display:block;font-size:12.5px;margin-top:2px;color:var(${err ? "--d-err" : "--d-muted"})"><code>${branch}</code>${text ? " " + text : ""}</span>`;
  // Remove from this machine: a quiet x at the row's end, and in the row's menu. Off, with why, on
  // the last machine and while an agent works there.
  const RM_X = '<svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg>';
  const rmb = (name, off) => off
    ? `<button class="dbtn quiet small rmx" disabled title="${off}" aria-label="Remove from ${name}">${RM_X}</button>`
    : `<button class="dbtn quiet small rmx" title="Remove from ${name}" aria-label="Remove from ${name}" data-dact="dset:pwr:confirm" data-dgo="${S.screen === "dWinProjectWindow" ? "dWinProjectWindow" : "dWinProject"}">${RM_X}</button>`;
  const chip = (cls, t, spin) => `<span class="pj-st${cls ? " " + cls : ""}">${spin ? '<span class="dspin"></span>' : ""}${t}</span>`;
  const pnum = n => `<span class="num">#${n}</span>`;
  const pick = id => `data-dact="dset:psel:${id}" data-dgo="${S.screen === "dWinProjectWindow" ? "dWinProjectWindow" : "dWinProject"}"`;
  const PJ_ISSUES = [
      { id: "is92", lead: pnum(92), t: "Auto-sync the SDK docs", sub: "opened by contributor-a · 32 d", tags: ["docs", "ci"] },
      { id: "is53", lead: pnum(53), t: "Publish the CLI like the SDK", sub: "opened by contributor-b · 173 d · 1 comment", tags: ["release"] }];
  const ISSUE_LABELS = ["ci", "docs", "release"];
  const LIST = [
    { title: "", items: [{ id: "overview", t: "Overview", sub: "2 running · 2 PRs · 2 issues" }] },
    { title: "Needs you", items: [{ id: "card", t: "The project agent waits on you", sub: "1 card" }] },
    { title: "Running", items: [
      { id: "run0", open: "dAgentSession", lead: hc("claude"), t: "Swipeable approval strip", sub: "build-box · fix-approval", r: '<span class="cc-running"><span class="dspin" role="img" aria-label="Working"></span>now</span>' },
      { id: "run1", open: "dAgentSession", lead: hc("codex"), t: "Fix the device build script", sub: "studio · main", r: "3m ago" }] },
    { title: "Project", items: [
      { id: "actions", t: "Actions", sub: "1 failed" },
      { id: "machines", t: "Machines", sub: "2/3 · 1 terminal" },
      { id: "sync", t: "File sync", sub: "in sync" },
      { id: "issues", t: "Issues", sub: `${PJ_ISSUES.length} open` }] },
    { title: "Work", items: [
      { id: "tasks", t: "Tasks", sub: "2 running" },
      { id: "agents", t: "Agents", sub: "2 running" },
      { id: "branches", t: "Branches", sub: "3 with work" }] },
    { title: "Pull requests", labels: ["cache", "deps", "perf"], items: [
      { id: "pr41", lead: pnum(41), t: "Cut the cache warm-up in half", sub: "sam · 5 d · CI failing", tags: ["perf", "cache"], r: chip("blocked", "needs reply") },
      { id: "pr38", lead: pnum(38), t: "Bump the sync crate", sub: "you · 9 d · ready · CI passing", tags: ["deps"], r: chip("done", "ready") }] },
  ];
  const ITEMS = LIST.flatMap(g => g.items);
  const md = s => `<div class="pj-md">${s}</div>`;
  const post = (who, when, body, cls = "", extra = "") => `<div class="pj-post${cls}"><div class="by"><b>${who}</b> · ${when}${extra}</div>${md(body)}</div>`;
  const hostName = forgejo => forgejo ? "Forgejo" : "GitHub";
  const prHead = (title, n, by, when, tags, extra, acts) => `<div class="pj-dh"><h3>${title}</h3><div class="meta">${chip("running", "open")}<span>#${n} opened by <b>${by}</b> ${when}</span>${tags.map(l => `<i class="pj-tag">${l}</i>`).join("")}${extra}</div><div class="acts">${acts}</div></div>`;
  // Plan and Discuss need a server, so the issue details hide them without one.
  const foot = forgejo => `<div class="pj-cbox"><textarea class="field" rows="3" aria-label="Comment" placeholder="Comment"></textarea><div class="row"><button class="dbtn pri" data-toast="Comment posted to ${hostName(forgejo)}">Comment</button></div></div>`;
  const details = {
    pr41: () => prHead("Cut the cache warm-up in half", 41, "sam", "5 days ago", ["perf", "cache"], chip("blocked", "changes requested") + chip("blocked", "checks failing"),
        `<span class="acts">${pb("Let the agent reply", "pri")}${pb("Continue")}${openReview("Review")}</span>${pb("Open on GitHub")}`)
      + `<div class="pj-db">${md("<p>The warm-up reads every entry twice. This reads the index once and fills the rest on demand.</p><ul><li>Cold start: 2.4 s to 1.1 s</li><li>No change to the on-disk format</li></ul><p>Run <code>cargo bench -p cache</code> to compare.</p>")}
        ${post("kim", "4 days ago", "<p>The lazy path skips the checksum. Can we keep it on first read?</p>")}${post("sam", "3 days ago", "<p>Good catch, fixed in the last push.</p>")}
        ${post("kim", "2 days ago", "<p>Cold start looks right here. The bench job still fails on main, unrelated?</p>")}${post("sam", "1 day ago", "<p>Unrelated, it fails without this branch too.</p>")}
        ${post("kim", "38 min ago", "<p>This changes when the index file is opened. Does the watcher still see a rename during warm-up?</p>", S.d.nfc ? " nf-on" : "", S.d.nfc ? pb("Open on GitHub") : "")}${foot(false)}</div>`,
    pr38: () => prHead("Bump the sync crate", 38, "you", "9 days ago", ["deps"], chip("running", "approved") + chip("done", "checks passing"),
        `<span class="acts">${pb("Continue", "pri")}${openReview("Review")}</span>${pb("Open on GitHub")}`)
      + `<div class="pj-db">${md("<p>Picks up the fix for the watcher on rename.</p>")}${foot(false)}</div>`,
    is92: () => prHead("Auto-sync the SDK docs", 92, "contributor-a", "32 days ago", ["docs", "ci"], "",
        `<span class="acts">${pb("Work on it", "pri")}</span>${pb(`Open on ${hostName(S.d.pjf)}`)}`)
      + `<div class="pj-db">${md("<p>The SDK docs are built by hand before each release. A workflow could build them on every tag.</p>")}${post("contributor-b", "30 days ago", "<p>+1, the docs for 0.13 are still missing.</p>")}${foot(S.d.pjf)}</div>`,
    is53: () => prHead("Publish the CLI like the SDK", 53, "contributor-b", "173 days ago", ["release"], "",
        `<span class="acts">${pb("Work on it", "pri")}</span>${pb("Open on GitHub")}`)
      + `<div class="pj-db">${md("<p>The SDK publishes from a tag. The CLI still needs a release by hand.</p>")}${post("sam", "170 days ago", "<p>Same workflow should do.</p>")}${foot(false)}</div>`,
    // Reached from a notification: approved, and not in the list.
    pr36: () => `<div class="pj-dh"><h3>Drop the v1 cache reader</h3><div class="meta">${chip("", "merged")}<span>#36 opened by <b>sam</b> 2 days ago</span>${chip("done", "checks passing")}</div><div class="acts">${pb("Open on GitHub")}</div></div>
      <div class="pj-db">${md("<p>Nothing reads the v1 format since 0.12. This removes the reader and its tests.</p>")}${post("kim", "3 h", "<p>Approved.</p>")}${foot(false)}</div>`,
    card: () => csec("Needs you", "the project chat waits on these", `<div class="pj-needs"><div class="pj-plan"><b>Plan 3 tasks</b><small>Tasks</small><ol><li>Split the warm-up from the index read</li><li>Keep the checksum on first read</li><li>Add a bench for cold start</li></ol><div class="row">${pb("Approve", "pri small")}${pb("Allow for this session", "small")}${pb("Deny", "warn small")}</div></div></div>`),
    tasks: () => `<div class="pj-cols">${csec("Tasks", "2 running · 1 in review · 1 done", `<div class="cc-panel">
        <div class="pj-task"><span class="t">Swipeable approval strip</span>${chip("running", "running", true)}<span class="m"><code>fix-approval</code><span>Claude Code on build-box</span></span><span class="acts">${openDiff("small")}${pb("Pause", "quiet small")}</span></div>
        <div class="pj-task"><span class="t">Fix the device build script</span>${chip("running", "running", true)}<span class="m"><code>main</code><span>Codex on studio</span></span><span class="acts">${pb("Pause", "quiet small")}</span></div>
        <div class="pj-task"><span class="t">Bump the sync crate</span>${chip("review", "review")}<span class="m"><a href="#">PR #38</a><code>bump-sync</code></span><span class="acts">${openReview("Review", "pri small")}${pb("Merge…", "small")}${pb("Open PR", "small")}${openDiff("small")}${pb("Back to open", "quiet small")}</span></div>
        <div class="pj-task"><span class="t">Add a bench for cold start</span>${chip("done", "done")}<span class="m"><code>bench-cold</code></span><span class="acts">${openDiff("small")}${pb("Reopen", "quiet small")}</span></div></div>`)}
      ${csec("Workers", "2 running", `<div class="cc-panel pj-tree">
        <div class="pj-node"><span class="cc-ag beep">${IC_CHAT}</span><span class="t"><b>Project agent</b><span>Plans tasks, files issues, places workers</span></span></div>
        <div class="pj-node child link">${hc("claude")}<span class="t"><b>Swipeable approval strip</b><span>Claude Code on build-box · running 1 min</span></span><span class="acts">${pb("Stop", "quiet small")}</span><span class="out">Wired the strip to the approval card.</span></div>
        <div class="pj-node child link">${hc("codex")}<span class="t"><b>Fix the device build script</b><span>Codex on studio · running 4 min</span></span><span class="acts">${pb("Stop", "quiet small")}</span></div></div>`)}</div>`,
    agents: () => csec("Agents", 2, panel(ln(hc("claude"), "Swipeable approval strip", "build-box · fix-approval", "1 min", 'data-dgo="dAgentSession"') + ln(hc("codex"), "Fix the device build script", "studio · main", "4 min", 'data-dgo="dAgentSession"')))
      + csec("Custom agents", 1, `<div class="cc-panel pv-scroll"><div class="pj-task ca-row"><span class="t">${hc("claude").replace('cc-ag', 'cc-ag ca-ic')}<b>reviewer</b> <span class="ca-desc">Reads a diff and lists what is wrong</span></span><span class="m"><span>sonnet</span><code>.claude/agents/reviewer.md</code></span><span class="acts">${pb("Start", "small")}</span></div></div>`, pb("Reload", "quiet small"))
      + csec("Past sessions", 2, `<div class="cc-panel">
        <div class="pv-row" style="cursor:pointer" data-dgo="dAgentSession"><span class="t">Retry uploads when the token expires</span><span class="m" style="align-items:center">${pvMark("claude")}<span>ended</span><span>build-box</span><span>widget</span><span>2 h</span></span></div>
        <div class="pv-row" style="cursor:pointer" data-dgo="dAgentSession"><span class="t">Bump the toolchain</span><span class="m" style="align-items:center">${pvMark("codex")}<span>ended</span><span>studio</span><span>widget</span><span>1 d</span></span></div></div>`),
    branches: () => {
      const br = (name, marks, subject, acts) => `<div class="pj-task pj-br"><span class="t"><code>${name}</code>${marks}</span><span class="m"><span>${subject}</span></span>${acts ? `<span class="acts rv-acts">${acts}</span>` : ""}</div>`;
      const add = n => `<span class="br-add">+${n}</span>`, tag = t => `<span class="br-tag">${t}</span>`;
      return `<div><div class="cc-sec"><h4>Branches</h4><small>read on build-box</small>${pb("Refresh", "quiet small")}</div><div class="cc-panel">
        ${br("main", tag("default"), "Merge the sync crate bump · 1 h", "")}
        ${br("login-copy", add(3) + tag("worktree"), "Say the same thing for a wrong email · 2 h", pb("View diff", "quiet small") + pb("Open review · needs changes", "small") + pb("Review again", "quiet small"))}
        ${br("fix-approval", add(2) + tag("worktree") + tag("not pushed"), "Swipe to approve · 1 min", pb("View diff", "quiet small") + pb("Review", "pri small"))}
        ${br("bump-sync", add(1), "Bump the sync crate · 9 d", pb("Review", "pri small") + pb("Add worktree", "small") + pb("Other machine…", "quiet small"))}</div></div>`;
    },
    machines: () => S.d.pwr === "one" ? csec("Where it lives", "", `<div class="cc-mt cc-pt pj-where" style="grid-template-columns:fit-content(40%) minmax(0,1fr) auto"><span class="th">Machine</span><span class="th">Folder</span><span class="th"></span>
        <span class="nm"><b>build-box</b></span><span class="nm"><span class="os">/srv/project/widget</span>${gnote("main", "")}</span><span class="acts">${gpair(0, 0, false, false)}${pb("Copy path", "quiet small")}${pb("Start an agent here", "small")}${rmb("build-box", "A project stays on at least one machine.")}</span>
        <span class="nm"><b>laptop</b></span><span class="nm"><span class="os">Not on laptop</span></span><span class="acts"><button class="dbtn small" data-dact="dset:pa:form" data-dgo="dAddToMachine">Add</button></span>
        <span class="nm"><b>studio</b></span><span class="nm"><span class="os">Not on studio</span></span><span class="acts"><button class="dbtn small" data-dact="dset:pa:form" data-dgo="dAddToMachine">Add</button></span></div>`).replace("</h4>", "<small>on 1 of 3</small></h4>")
      : csec("Where it lives", "", `<div class="cc-mt cc-pt pj-where" style="grid-template-columns:fit-content(40%) minmax(0,1fr) auto"><span class="th">Machine</span><span class="th">Folder</span><span class="th"></span>
        <span class="nm"><b>build-box</b></span><span class="nm"><span class="os">/srv/project/widget</span>${gnote("main", "")}</span><span class="acts">${gpair(1, 2, true, true)}${pb("Copy path", "quiet small")}${pb("Start an agent here", "small")}${rmb("build-box", "An agent is working in acme/widget on build-box. Stop it first.")}</span>
        <span class="nm"><b>studio</b></span><span class="nm"><span class="os">/srv/widget</span> <small>Synced by Syncthing</small>${gnote("main", "")}</span><span class="acts">${gpair(0, 0, false, false)}${pb("Open in Finder", "quiet small")}${pb("Copy path", "quiet small")}${pb("Start an agent here", "small")}${rmb("studio")}</span>
        <span class="nm"><b>laptop</b></span><span class="nm"><span class="os">Not on laptop</span></span><span class="acts"><button class="dbtn small" data-dact="dset:pa:form" data-dgo="dAddToMachine">Add</button></span>
        <span class="nm"><b>mini-box</b></span><span class="nm"><span class="os">/srv/project/widget</span> <small>Syncing 42%</small>${gnote("fix-approval", "not pushed")}</span><span class="acts">${gpair(0, 0, false, true)}${pb("Copy path", "quiet small")}${pb("Start an agent here", "small")}${rmb("mini-box")}</span>
        <span class="nm"><b>old-box</b></span><span class="nm"><span class="os">/path/to/code/widget</span> <small>Already here</small>${gnote("main", "")}</span><span class="acts">${gpair(3, 0, true, false, "pull")}${pb("Copy path", "quiet small")}${pb("Start an agent here", "small")}${rmb("old-box")}</span>
        <span class="nm"><b>dev-vm</b></span><span class="nm"><span class="os">/path/to/widget</span>${gnote("main", "")}<span class="gs" style="display:block;font-size:12.5px;margin-top:2px;color:var(--d-err)">dev-vm: fatal: Not possible to fast-forward, aborting.</span></span><span class="acts">${gpair(2, 1, true, true)}${gb("Fix with an agent", "small", false)}${pb("Copy path", "quiet small")}${pb("Start an agent here", "small")}${rmb("dev-vm")}</span>
        <span class="nm"><b>nas</b></span><span class="nm"><span class="os">/srv/widget</span><span class="gs" style="display:block;font-size:12.5px;margin-top:2px;color:var(--d-muted)">Detached HEAD, nothing to pull or push</span></span><span class="acts">${pb("Copy path", "quiet small")}${pb("Start an agent here", "small")}${rmb("nas")}</span>
        <span class="nm"><b>win-box</b></span><span class="nm"><span class="os">C:\\Users\\user\\code\\widget</span><br><small class="gs err" title="The folder is gone. Beep makes it again and fills it from the other machines.">Folder missing: filling it again from the other machines</small></span><span class="acts">${pb("Copy path", "quiet small")}${pb("Start an agent here", "small")}${rmb("win-box")}</span>
        <span class="nm"><b>gpu-box</b></span><span class="nm"><span class="os">gpu-box shares no folders</span></span><span class="acts">${pb("Clone here", "small")}</span></div>`).replace("</h4>", "<small>on 7 of 9</small></h4>"),

    sync: () => S.d.fss === "stuck" ? `<div class="cc-sec"><h4>Machines</h4><span class="fs-tools"><small>1 of 5 in sync · 3 not syncing · 1 offline</small>${pb("Pause")}</span></div>
      <div class="cc-panel fs-list">
        <div class="fs-row" title="Right-click: Change folder…"><div class="fs-h"><span class="ddot"></span><b>studio</b><span class="pb-chip wait" title="/srv/share/.stignore has rules Beep cannot follow, so it does not take over from Syncthing: the directive #include">not syncing</span></div><div class="fs-m"><span>0 files</span><code>/srv/widget</code></div><div class="cc-err">/srv/share/.stignore has rules Beep cannot follow, so it does not take over from Syncthing: the directive #include</div></div>
        <div class="fs-row"><div class="fs-h"><span class="ddot"></span><b>build-box</b><span class="pb-chip wait">Git sync needs review</span></div><div class="fs-m"><span>Git sync</span><code>/srv/project/widget</code></div><div class="cc-err">Git histories diverged. Review both sides before merging.</div><div class="fs-m">${pb("Fix with an agent", "small")}</div><div class="fs-m">Opens a draft with both checkouts and recent session references. The agent can read transcripts from either machine.</div></div>
        <div class="fs-row"><div class="fs-h"><span class="ddot"></span><b>mini-box</b><span class="pb-chip wait">Local edits block Git sync</span></div><div class="cc-err">The checkout has staged, unstaged or untracked changes. Review them before updating.</div><div class="fs-m">${pb("Fix with an agent", "small")}</div></div>
        <div class="fs-row"><div class="fs-h"><span class="ddot"></span><b>win-box</b><span class="pb-chip wait">folder missing</span></div><div class="fs-m"><span>0 files</span><code>C:\\Users\\user\\code\\widget</code></div><div class="cc-err">The folder is gone. Beep makes it again and fills it from the other machines.</div></div>
        <div class="fs-row"><div class="fs-h"><span class="ddot"></span><b>nas</b><span class="pb-chip wait">disk missing</span></div><div class="fs-m"><span>0 files</span><code>/mnt/disk/widget</code></div><div class="cc-err">The folder above it is gone too, so nothing is made. Is its disk mounted?</div></div>
        <div class="fs-row"><div class="fs-h"><span class="ddot off"></span><b>laptop</b><span class="pb-chip">offline</span></div><div class="fs-m"></div></div></div>
      <div class="dmuted fs-note">Builds, ignored files and links stay on each machine. Right-click a machine to change its folder.</div>`
    : `<div class="cc-sec"><h4>Machines</h4><span class="fs-tools"><small>in sync</small>${pb("Pause")}</span></div>
      <div class="cc-panel fs-list">
        <div class="fs-row"><div class="fs-h"><span class="ddot"></span><b>build-box</b><span class="pb-chip">in sync</span></div><div class="fs-m"><span>1,880 files</span><span>284 MB</span><code>/srv/project/widget</code></div></div>
        <div class="fs-row"><div class="fs-h"><span class="ddot"></span><b>studio</b><span class="pb-chip">in sync</span></div><div class="fs-m"><span>1,880 files</span><span>284 MB</span><code>/srv/widget</code></div><div class="fs-m"><span class="clockoff" title="Sync keeps the newer file when two machines edit one at once, so turn on automatic time sync in Date and Time settings.">Clock is 2 min behind</span></div></div>
        <div class="fs-row"><div class="fs-h"><span class="ddot off"></span><b>laptop</b><span class="pb-chip">offline</span></div><div class="fs-m"></div></div></div>
      <div class="cc-sec"><h4>Conflicts</h4><small>edited on two machines at once</small></div>
      <div class="cc-panel fs-list">${fsConflict("notes (conflict 1a2b3c4d).txt", "Another copy of notes.txt · on build-box, studio")}${fsConflict(".git/refs/beep-conflicts/heads/main-5e6f7a8b", "Another commit of main · on build-box")}</div>
      <div class="dmuted fs-note">Builds, ignored files and links stay on each machine.</div>`,
  };
  const CHECK_MARK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 7.5"/></svg>';
  const FAIL_MARK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round"><path d="M7 7l10 10M17 7L7 17"/></svg>';
  // The run opened in Actions: its jobs, steps, the end of the log and Fix this.
  const runOpen = (who) => `<span class="pj-run"><span class="pj-jobs"><b class="bad">test</b><small>failed, 3 min</small><b>lint</b><small>passed, 48 s</small></span>
    <span class="pj-steps"><span class="pj-step ok"><span class="ddot off"></span><span>1. Check out</span><small>4 s</small></span><span class="pj-step bad"><span class="ddot bad"></span><span>2. cargo test</span><small>3 min</small></span><span class="pj-step skip"><span class="ddot off"></span><span>3. Upload results</span><small>skipped</small></span></span>
    <span class="pj-log"><small>End of the log of test, cargo test</small><pre>error[E0061]: this function takes 2 arguments but 1 argument was supplied
   at src/index/warm.rs:212:18
error: could not compile \`cache\` (lib) due to 1 previous error
Error: Process completed with exit code 101.</pre></span>
    <span class="acts">${pb("Fix this", "pri")}${pb("Full log")}</span></span>`;
  const runRow = (n, title, branch, who, when, took, ok, open) => `<div class="pj-runrow ${ok ? "ok" : "bad"}${open ? " open" : ""}"><div class="rr-head" role="button" tabindex="0" aria-expanded="${!!open}" ${pick(open ? "actions" : "ci" + n)}>
      <span class="rr-st ${ok ? "ok" : "bad"}">${ok ? CHECK_MARK : FAIL_MARK}</span>
      <div class="rr-body"><div class="rr-t"><b>CI #${n}</b><span>${title}</span></div><div class="rr-m"><code class="br">${branch}</code><span class="ev">push</span><span class="ev">${who}</span></div></div>
      <div class="rr-side"><span class="rr-when">${when}</span><span class="rr-took">${took}</span></div><span class="rr-acts">${pb("Open", "quiet small")}</span><span class="rr-chev${open ? " on" : ""}"></span></div>${open ? runOpen(who) : ""}</div>`;
  const actions = open => `<div class="pj-actions"><div class="cc-sec"><h4>Actions</h4><small>2 failed, 1 passed</small><small class="sw-age">synced 1 min ago</small>${pb("Refresh", "quiet small")}</div>
    <div class="cc-sec"><h4>Running</h4><small>1</small></div>
    <div class="cc-panel pj-runbox">${runRow(353, "Fix the device build script", "main", "you", "1 min ago", "running", true, false).replace(/<span class="rr-st ok">.*?<\/span>/, '<span class="rr-st running"><span class="ddot"></span></span>')}</div>
    <div class="cc-sec"><h4>Finished</h4><small>3</small></div>
    <div class="pj-labels"><button class="pj-tag lab" data-toast="Filter: failed">failed 2</button><button class="pj-tag lab" data-toast="Filter: passed">passed 1</button></div>
    <div class="cc-panel pj-runbox">${runRow(351, "Cut the cache warm-up in half", "main", "sam", "12 min ago", "3m 12s", false, open === 351)}${runRow(349, "Retry uploads when the token expires", "main", "kim", "5 h ago", "2m 41s", false, open === 349)}${runRow(347, "Bump the sync crate", "bump-sync", "you", "9 d ago", "2m 30s", true, false)}</div></div>`;
  details.actions = () => actions(0);
  details.ci351 = () => actions(351);
  details.ci349 = () => actions(349);
  // The project's Overview: a box per list, each title opening that row.
  details.overview = () => {
    const box = (title, count, id, rows) => csec(`<button class="lk" ${pick(id)}>${title}</button>`, count, panel(rows));
    const rowsOf = g => (g === "Issues" ? PJ_ISSUES : LIST.find(x => x.title === g).items).map(i => ln(i.lead, i.t, i.sub, i.r || "", i.open ? `data-dgo="${i.open}"` : pick(i.id))).join("");
    const dot = off => `<span class="ddot${off ? " off" : ""}"></span>`;
    const tagged = i => [i.r || "", ...(i.tags || []).slice(0, 3).map(l => `<i class="pj-tag">${l}</i>`)].join("");
    const scroll = (rows, go) => rows.map(i => ln(i.lead, i.t, i.sub, tagged(i), pick(i.id))).join("");
    return `<div class="pj-ov"><div>${box("Running", 2, "agents", rowsOf("Running"))}
      ${csec("Past sessions", 2, panel(ln("", "Retry uploads when the token expires", "Claude Code · build-box · ended 2 h ago", "") + ln("", "Bump the sync crate", "Codex · studio · ended 9 d ago", "")))}
      ${box("Actions", "1 running · 1 failed", "actions", ln("", "Fix the device build script", "build.yml #353", chip("running", "running", true), pick("actions")) + ln("", "Cut the cache warm-up in half", "build.yml #351", chip("blocked", "failed"), pick("ci351")) + ln("", "Bump the sync crate", "build.yml #347", chip("done", "passed"), pick("actions")))}
      ${box("Machines", "2/3", "machines", ln(dot(), "build-box", "main", "", pick("machines")) + ln(dot(), "studio", "fix-approval", "", pick("machines")) + ln(dot(true), "laptop", "not cloned", "", pick("machines")))}
      ${box("Terminals", 1, "machines", ln(dot(), "cargo watch", "build-box · acme/widget · ~/code/widget", "3 h", 'data-dact="dset:ags:drafterm" data-dgo="dAgentSession"'))}</div>
      <div>${box("Pull requests", 2, "pr41", rowsOf("Pull requests"))}
      ${csec(`<button class="lk" ${pick("issues")}>Issues</button>`, PJ_ISSUES.length, `<div class="cc-panel pj-scroll">${scroll(PJ_ISSUES)}</div>`)}</div></div>`;
  };
  const pjRow = (i, lit) => `<button class="pj-li" title="${i.t}"${i.id === lit ? ' aria-current="true"' : ""} ${i.open ? `data-dgo="${i.open}"` : pick(i.id)}><span class="lead">${i.lead || ""}</span><span class="t"><b>${i.t}</b><span>${i.sub}${(i.tags || []).map(l => `<i class="pj-tag lab">${l}</i>`).join("")}</span></span>${i.r ? `<span class="r">${i.r}</span>` : ""}</button>`;
  // Issues list at the right; one issue opens there with a way back.
  details.issues = () => __listCtl("pji", true) + `<div class="pj-labels">${ISSUE_LABELS.map(l => `<button class="pj-tag lab" data-toast="Filter: ${l}">${l}</button>`).join("")}</div><div class="pj-rows">${PJ_ISSUES.map(i => S.d.pjis ? `<div class="pj-prow"><label><input type="checkbox"${i.id === "is92" ? " checked" : ""} aria-label="Pick #${i.t}"></label>${pjRow(i)}</div>` : pjRow(i)).join("")}</div>` + __listStart("pji", [["acme/widget", "#92 Auto-sync the SDK docs", ""]]);
  for (const id of ["is92", "is53"]) {
    const page = details[id];
    details[id] = () => `<button class="dbtn quiet small pj-back" ${pick("issues")}>All issues</button>` + page();
  }
  // Which row a detail keeps lit.
  const LIT = { ci351: "actions", ci349: "actions", review: "branches", is92: "issues", is53: "issues", terminals: "machines" };
  // Machines and their terminals are one pane, as on Home.
  const details_terminals = () => {
    // Terminals keep running on their machines; a row opens the agent view with it in a pane tab.
    const none = S.d.ptm === "none", open = 'data-dact="dset:ags:drafterm" data-dgo="dAgentSession"';
    const row = (dotOff, name, sub, right) => `<div class="cc-row cc-line" ${open}><span class="ddot${dotOff ? " off" : ""}"></span><button class="t" title="${name}"><b>${name}</b><span>${sub}</span></button><span class="r">${right}</span></div>`;
    const rows = none ? "" : row(false, "cargo watch", "build-box · ~/code/widget", "running · 3 h") + row(false, "widget 1", "build-box · ~/code/widget", "running · 2 min") + row(true, "shell", "studio · ~/code/widget", "studio is offline, it may still run");
    return `<div class="cc-sec"><h4>Terminals${none ? "" : ' <span style="color:var(--d-muted);font-weight:500;letter-spacing:0">3</span>'}</h4><small>Cmd T opens a new one</small></div>`
      + (none ? '<div class="cc-panel cc-empty"><b>No terminals</b><span>Press Cmd T to open one in the project.</span></div>' : `<div class="cc-panel">${rows}</div>`);
  };
  const whereOnly = details.machines;
  details.machines = details.terminals = () => whereOnly() + details_terminals();
  details.review = () => window.__rvPane();
  window.__pjReviewPage = () => { const keep = S.d.psel; S.d.psel = "review"; try { return D.dWinProject(); } finally { S.d.psel = keep; } };
  const openDiff = (cls = "") => `<button class="dbtn${cls ? " " + cls : ""}" data-dact="dset:ags:diffzoom" data-dgo="dAgentSession">View diff</button>`;
  const openReview = (l, cls = "") => `<button class="dbtn${cls ? " " + cls : ""}" data-dact="dset:psel:review" data-dgo="dWinProject">${l}</button>`;
  const CHEV = '<svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';
  const pjMenu = () => `<div class="dmenu" style="left:auto;top:44px;right:12px;z-index:9;width:200px"><button class="mi" data-toast="Pin"><b>Pin</b></button>${S.screen === "dWinProjectWindow" ? "" : '<button class="mi" data-dgo="dWinProjectWindow"><b>Open in New Window</b></button><button class="mi" data-dgo="dWinProjects"><b>All projects</b></button>'}<button class="mi" data-dact="dset:igsform:1" data-dgo="dWinProject"><b>Sync gitignored: ${S.d.igs ? "On" : "Off"}</b></button><button class="mi" data-dact="dset:igsform:1" data-dgo="dWinProject"><b>Choose ignored folders…</b></button><button class="mi" data-toast="Remove project"><b style="color:var(--d-err)">Remove project…</b></button></div>`;
  // The composer always names a real machine: the last picked while it has a checkout and an
  // agent, else the first such machine. With none, it reads "Add to a machine" and picking one
  // adds the project there (the machine row's Add), the progress on the line under it.
  const pjComp = () => {
    const add = S.d.pjadd || "", none = add === "menu" || add === "none", adding = add === "adding";
    const machines = [["build-box", "Claude Code, Codex"], ["mac-box", "Claude Code"]];
    const menu = add === "menu" ? `<div class="dmenu" style="top:auto;bottom:calc(100% + 6px);width:240px">${machines.map(([m, a]) => `<button class="mi" data-dact="dset:pjadd:adding" data-dgo="dWinProject"><b>${m}</b><small>${a}</small></button>`).join("")}</div>` : "";
    const pill = none ? `<span style="position:relative"><button class="pill" type="button" data-dact="dset:pjadd:${add === "menu" ? "none" : "menu"}" data-dgo="dWinProject">Add to a machine ${CHEV}</button>${menu}</span>`
      : `<button class="pill pj-machine" type="button" title="Shift+Tab switches machines"${adding ? " disabled" : ""}>${S.d.pjmachine || 'build-box'}${adding ? " <small>adding</small>" : ""}</button>`;
    const ph = none || adding ? "Add acme/widget to a machine to start an agent" : "Describe a task for a coding agent in acme/widget";
    const err = adding ? `<span class="cc-err" role="status" data-dact="dset:pjadd:" data-dgo="dWinProject">Copying to build-box: 1,204 of 1,880 files</span>` : "";
    // Images pasted or dropped on the input go with the first prompt, as in a session's field.
    const att = S.d.pjimg ? `<div class="ag-att"><span><i></i>Screenshot 2026-10-07.png<em data-dact="dset:pjimg:" data-dgo="dWinProject">&#215;</em></span></div>` : "";
    return `<form class="cc-pjc" onsubmit="return false"><div class="box">${att}<textarea rows="1" placeholder="${ph}">${S.d.pjimg ? "The login button overlaps the field, as in this screenshot" : ""}</textarea><div class="row">${pill}<button class="pill" type="button"${none || adding ? " disabled" : ""}>Current checkout <small>as it is</small></button><span class="sp"></span><button class="pill" type="button">${hc("claude")}Claude Code <small>Default · Medium</small></button><button class="dbtn pri" type="button" data-toast="Started"${none || adding ? " disabled" : ""}>Start<kbd>⏎</kbd></button></div></div>${err}</form>`;
  };
  document.addEventListener('keydown', e => {
    if (e.key !== 'Tab' || !e.shiftKey || e.altKey || e.ctrlKey || e.metaKey || e.isComposing || !e.target.matches('.cc-pjc textarea')) return;
    const pill = e.target.closest('.cc-pjc').querySelector('.pj-machine:not(:disabled)');
    if (!pill) return;
    e.preventDefault();
    S.d.pjmachine = pill.textContent === 'build-box' ? 'mac-box' : 'build-box';
    pill.textContent = S.d.pjmachine;
  });
  D.dWinProject = () => {
    const sel = S.d.psel || "overview", lit = LIT[sel] || sel;
    const list = LIST.map(g => (g.title ? `<h4>${g.title}<span class="n">${g.items.length}</span></h4>` : "")
      + (g.title === "Pull requests" ? __listCtl("pjp") : "")
      + (g.labels ? `<div class="pj-labels">${g.labels.map(l => `<button class="pj-tag lab" data-toast="Filter: ${l}">${l}</button>`).join("")}</div>` : "")
      + g.items.map(i => pjRow(i, lit)).join("")).join("");
    const forgejo = S.d.pjf && sel === "is92";
    const forges = `<button class="dbtn small" title="github.com/acme/widget" data-toast="Opens github.com/acme/widget">GitHub</button><button class="dbtn small" title="forge.example.com/acme/widget" data-toast="Opens forge.example.com/acme/widget">Forgejo</button>`;
    const main = `<div class="nb-main" style="grid-template-rows:auto minmax(0,1fr) auto"><div class="cc-top"><h2>acme/widget</h2><small>${forgejo ? "forgejo.example.com" : "github.com"} · imported 3 d</small><span class="tools">${forges}<span class="ed-tools">${pb("Open in Zed", "quiet small")}<button class="dbtn quiet small" aria-label="Choose an editor" data-toast="Editors">${CHEV}</button></span><button class="dbtn quiet small" aria-label="Project menu" data-dact="dset:pmenu:${S.d.pmenu ? "" : "1"}" data-dgo="dWinProject">⋯</button></span></div>
      <div class="cc-view split"><div class="pj-split"><div class="pj-list">${list}</div><div class="pj-det">${details[sel]()}</div></div></div>
      ${pjComp()}${S.d.pmenu ? pjMenu() : ""}</div>`;
    const page = skinned(ccWin(ccSidePj("acme/widget"), main));
    if (S.d.igsform === "1") return ccSheetOver(page, `<div class="am-head"><b>Sync gitignored</b><kbd>esc</kbd></div>
      <label class="am-line"><input type="checkbox" checked>Sync selected ignored folders on all machines</label>
      <label class="am-label" for="ignored-folders">Folders relative to the project, one per line</label><textarea id="ignored-folders" class="ip-path" rows="4">assets/models
data/captures</textarea>
      <div class="am-sub">Each folder must be wholly ignored by Git and contain no tracked or staged files. These folders use file sync, including conflict copies. Other uncommitted edits stay local. Build output, linked repositories and paths excluded in .beepignore stay local.</div>
      <div class="am-line ip-acts"><button class="dbtn quiet" data-dact="dset:igsform:" data-dgo="dWinProject">Cancel</button><button class="dbtn pri" data-dact="dset:igs:1" data-dgo="dWinProject" data-toast="Ignored-folder sync saved for all machines" onclick="S.d.igsform=''">Save</button></div>`);
    if (sel === "sync" && S.d.fss === "change") {
      // Change folder…, from a machine's row menu: another folder there, nothing moved.
      return ccSheetOver(page, `<div class="am-head"><b>Change the folder of widget on win-box</b><kbd>esc</kbd></div>
        <div class="am-sub">Now in <code>C:\\Users\\user\\code\\widget</code></div>
        <div class="am-label">Folder on win-box</div>
        <div class="am-line"><div class="ip-folder"><input class="ip-path ip-dir" value="/d/work/widget"></div><button class="dbtn quiet">Choose…</button></div>
        <div class="am-sub">Pick an empty folder, which the other machines fill, or a checkout of the same repository, which joins the sync. The folder it is in now stays on disk as it is.</div>
        <div class="am-line ip-acts"><button class="dbtn quiet" data-dact="dset:fss:stuck" data-dgo="dWinProject">Cancel</button><button class="dbtn pri ip-go" data-dact="dset:fss:stuck" data-dgo="dWinProject" data-toast="widget on win-box now in D:\\work\\widget">Change</button></div>`);
    }
    if (sel !== "machines" || (S.d.pwr !== "confirm" && S.d.pwr !== "refused" && S.d.pwr !== "delete")) return page;
    // Remove from this machine asks first. The node there stops syncing; its folder stays as it is.
    const back = S.screen === "dWinProjectWindow" ? "dWinProjectWindow" : "dWinProject";
    const err = S.d.pwr === "refused" ? `<div class="am-msg bad">An agent is working in acme/widget on mini-box. Stop it first.</div>` : "";
    const del = S.d.pwr === "delete";
    return ccSheetOver(page, `<div class="am-head"><b>Remove acme/widget from mini-box?</b><kbd>esc</kbd></div>
      <div class="am-msg">It stops syncing to mini-box and leaves the project's machines.</div>
      <label class="am-sub" style="display:flex;gap:8px;align-items:center"><input type="checkbox"${del ? " checked" : ""} data-dact="dset:pwr:${del ? "confirm" : "delete"}" data-dgo="${back}">Also delete the folder on mini-box</label>
      <div class="am-sub"><code>/srv/project/widget</code> ${del ? "is deleted on mini-box once it stops syncing there. The other machines keep their copies." : "stays on disk as it is."}</div>${err}
      <div class="gp-foot"><button class="dbtn quiet" data-dact="dset:pwr:" data-dgo="${back}">Cancel</button><button class="dbtn warn" data-dact="dset:pwr:" data-dgo="${back}" data-toast="${del ? "acme/widget removed from mini-box. Deleted /srv/project/widget there" : "acme/widget removed from mini-box. Its folder stays"}">${del ? "Remove and delete" : "Remove"}</button></div>`);
  };
  VARIANTS.dWinProject = [
    ["Overview", () => { S.d.psel = "overview"; S.d.pmenu = ""; S.d.pjadd = ""; S.d.pjimg = false; render(); }, () => (S.d.psel || "overview") === "overview" && !S.d.pmenu && !S.d.pjadd && !S.d.pjimg],
    ["Composer: no checkout on an agent machine", () => { S.d.psel = "overview"; S.d.pjadd = "menu"; render(); }, () => S.d.pjadd === "menu" || S.d.pjadd === "none"],
    ["Composer: adding to a machine", () => { S.d.psel = "overview"; S.d.pjadd = "adding"; render(); }, () => S.d.pjadd === "adding"],
    ["Composer: an image attached", () => { S.d.psel = "overview"; S.d.pjadd = ""; S.d.pjimg = true; render(); }, () => !!S.d.pjimg],
    ["Pull request", () => { S.d.psel = "pr41"; S.d.nfc = ""; render(); }, () => S.d.psel === "pr41" && !S.d.nfc],
    ["Issues", () => { S.d.psel = "issues"; render(); }, () => S.d.psel === "issues"],
    ["Issue", () => { S.d.psel = "is92"; S.d.pjf = false; render(); }, () => S.d.psel === "is92" && !S.d.pjf],
    ["Issue on Forgejo", () => { S.d.psel = "is92"; S.d.pjf = true; render(); }, () => S.d.psel === "is92" && !!S.d.pjf],
    ["Needs you", () => { S.d.psel = "card"; render(); }, () => S.d.psel === "card"],
    ["Tasks", () => { S.d.psel = "tasks"; render(); }, () => S.d.psel === "tasks"],
    ["Agents", () => { S.d.psel = "agents"; render(); }, () => S.d.psel === "agents"],
    ["Branches", () => { S.d.psel = "branches"; render(); }, () => S.d.psel === "branches"],
    ["Review", () => { S.d.psel = "review"; render(); }, () => S.d.psel === "review"],
    ["Actions", () => { S.d.psel = "actions"; render(); }, () => S.d.psel === "actions"],
    ["Machines", () => { S.d.psel = "machines"; S.d.pwr = ""; render(); }, () => S.d.psel === "machines" && !S.d.pwr],
    ["Machines: remove from one", () => { S.d.psel = "machines"; S.d.pwr = "confirm"; render(); }, () => S.d.psel === "machines" && S.d.pwr === "confirm"],
    ["Machines: removal refused", () => { S.d.psel = "machines"; S.d.pwr = "refused"; render(); }, () => S.d.psel === "machines" && S.d.pwr === "refused"],
    ["Machines: remove and delete the folder", () => { S.d.psel = "machines"; S.d.pwr = "delete"; render(); }, () => S.d.psel === "machines" && S.d.pwr === "delete"],
    ["Machines: on one machine", () => { S.d.psel = "machines"; S.d.pwr = "one"; render(); }, () => S.d.psel === "machines" && S.d.pwr === "one"],
    ["Terminals", () => { S.d.psel = "terminals"; S.d.ptm = ""; render(); }, () => S.d.psel === "terminals"],
    ["File sync", () => { S.d.psel = "sync"; S.d.fss = ""; render(); }, () => S.d.psel === "sync" && !S.d.fss],
    ["File sync: not syncing, folder missing", () => { S.d.psel = "sync"; S.d.fss = "stuck"; render(); }, () => S.d.psel === "sync" && S.d.fss === "stuck"],
    ["File sync: Change folder", () => { S.d.psel = "sync"; S.d.fss = "change"; render(); }, () => S.d.psel === "sync" && S.d.fss === "change"],

    ["Project menu", () => { S.d.psel = "overview"; S.d.pmenu = "1"; render(); }, () => !!S.d.pmenu],
    ...skinVariants].map(([l, f, on]) => l === "Project menu" ? [l, f, on] : [l, () => { S.d.pmenu = ""; f(); }, on]);
  // A project with no project agent: its sessions, where it lives, what runs there.
  window.__pjOne = () => `<div class="pj-cols pj-half">${csec("Past sessions", 2, `<div class="cc-panel"><div class="pv-row" style="cursor:pointer" data-dgo="dAgentSession"><span class="t">Retry uploads when the token expires</span><span class="m" style="align-items:center">${pvMark("claude")}<span>ended</span><span>build-box</span><span>widget</span><span>2 h</span></span></div><div class="pv-row" style="cursor:pointer" data-dgo="dAgentSession"><span class="t">Bump the toolchain</span><span class="m" style="align-items:center">${pvMark("codex")}<span>ended</span><span>studio</span><span>widget</span><span>1 d</span></span></div></div>`)}<div class="ca-col">${details.machines()}${csec("Agents", "", '<div class="cc-panel cc-empty"><span>No agent is working in this project right now.</span></div>')}${csec("Custom agents", "", '<div class="cc-panel cc-empty"><span>No custom agents in .claude/agents, .opencode/agent, .codex/agents, .pi/agents or .agents yet.</span></div>', pb("Reload", "quiet small"))}</div></div>`;

  // ---- Settings, Appearance: the skin picker ----
  const SK = [["Linear", "#151617"], ["Classic", "#1a2a23"], ["Quiet", "#161b18"], ["GitHub dark", "#161b22"], ["Zed", "#2f343e"], ["Tokyo Night", "#1f2335"], ["Catppuccin Mocha", "#313244"], ["Nord", "#3b4252"], ["Gruvbox", "#3c3836"], ["Rose Pine", "#26233a"], ["Geist", "#000"], ["High contrast", "#000"]];
  const SKL = [["Light", "#f3f3f1"], ["Solarized", "#eee8d5"], ["macOS", "#e9e7ea"], ["Warm", "#f3efe8"]];
  const on = () => S.d.skin === "paper" ? "Light" : "Linear";
  const sw = list => list.map(([n, c]) => `<button class="nb-skin" aria-pressed="${n === on()}" ${n === "Linear" || n === "Light" ? `data-dact="dset:skin:${n === "Light" ? "paper" : ""}" data-dgo="dAppearance"` : `data-toast="Skin: ${n}"`}><i style="background:${c}"></i>${n}</button>`).join("");
  D.dAppearance = () => { const keep = S.d.set; S.d.set = "appearance"; const html = D.dWinSettings(); S.d.set = keep; return skinned(html); };
  VARIANTS.dAppearance = skinVariants;
  DESK_SCREENS.find(g => g.group === "Command center").items.push(["dAppearance", "Settings, Appearance", "SettingsSheet, SkinPicker"]);
}
