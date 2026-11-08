
// Start form as a composer: prompt, then one pill for harness, model and
// thinking, with Machine and Folder dropdowns under the box.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .cc-view:has(> .agc) { display: flex; flex-direction: column }
.mac .agc { max-width: 760px; width: 100%; margin: auto auto 18px; display: grid; gap: 10px }
.mac .agc h3 { margin: 0 0 14px; text-align: center; font-size: 26px; font-weight: 500; letter-spacing: -.01em }
.mac .agc h3 b { font-weight: 600 }
.mac .agc-box { position: relative; border: 1px solid var(--d-border); border-radius: 16px; background: var(--d-surface); padding: 14px 14px 10px }
.mac .agc-box .pr { min-height: 58px; font-size: 14.5px; line-height: 21px; color: var(--d-fg) }
.mac .agc-box .pr.ph { color: var(--d-dim) }
.mac .agc-row { display: flex; align-items: center; gap: 8px }
.mac .agc-row .sp { flex: 1 }
.mac .agc-pill { all: unset; cursor: pointer; display: inline-flex; align-items: center; gap: 8px; padding: 5px 10px; border-radius: 9px; font-size: 13px; color: var(--d-fg) }
.mac .agc-pill:hover, .mac .agc-pill[aria-expanded="true"] { background: rgba(227,228,230,.06) }
.mac .agc-pill small { color: var(--d-muted); font-size: 12.5px }
.mac .agc-pill svg { color: var(--d-muted) }
.mac .agc-send { all: unset; cursor: pointer; width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center; background: var(--d-emerald); color: #ffffff }
.mac .agc-send[disabled] { background: var(--d-elev); color: var(--d-dim); cursor: default }
.mac .agc-under { display: flex; align-items: center; gap: 4px; padding: 0 6px; font-size: 12.5px; color: var(--d-muted); position: relative }
.mac .agc-under .agc-pill { font-size: 12.5px; color: var(--d-muted); padding: 4px 8px }
.mac .agc-under .sep { width: 1px; height: 14px; background: var(--d-border) }
.mac .agc-why { display: flex; gap: 8px; align-items: center; padding: 8px 12px; border-radius: 8px; border: 1px solid var(--d-border); font-size: 12.5px; color: var(--d-muted) }
.mac .agc-why b { color: var(--d-fg); font-weight: 500 }
.mac .agc-pick { position: absolute; right: 54px; bottom: 52px; z-index: 8; width: min(640px, 100%); max-height: 380px; display: grid; grid-template-columns: 58px 1fr 184px; border: 1px solid var(--d-border); border-radius: 14px; background: #0f1011; box-shadow: 0 18px 50px rgba(0,0,0,.5); overflow: hidden }
.mac .agc-rail { display: grid; align-content: start; gap: 8px; padding: 12px 11px; border-right: 1px solid var(--d-border) }
.mac .agc-rail button { all: unset; cursor: pointer; width: 34px; height: 34px; border-radius: 9px; display: grid; place-items: center; position: relative }
.mac .agc-rail button[aria-pressed="true"] { background: rgba(94,106,210,.12) }
.mac .agc-rail button[aria-pressed="true"]::after { content: ""; position: absolute; right: -9px; top: 8px; bottom: 8px; width: 2px; border-radius: 2px; background: var(--d-emerald) }
.mac .agc-rail button[disabled] { opacity: .35; cursor: default }
.mac .agc-list { display: grid; align-content: start; max-height: 360px; overflow-y: auto; scrollbar-width: thin; padding-bottom: 8px }
.mac .agc-find { position: sticky; top: 0; z-index: 1; padding: 12px 12px 8px; background: #0f1011 }
.mac .agc-search { padding: 8px 12px; border: 1px solid var(--d-border); border-radius: 9px; background: rgba(227,228,230,.04); font-size: 13px; color: var(--d-dim) }
.mac .agc-list button { all: unset; cursor: pointer; display: flex; align-items: center; gap: 8px; min-height: 22px; margin: 0 6px; padding: 6px 10px; border-radius: 8px; font-size: 13px }
.mac .agc-list button:hover { background: rgba(255,255,255,.04) }
.mac .agc-list button.on { background: rgba(94,106,210,.12) }
.mac .agc-list button small { color: var(--d-muted); font-size: 11.5px; margin-left: auto }
.mac .agc-think { border-left: 1px solid var(--d-border); padding: 14px 0 10px }
.mac .agc-think h5 { margin: 0 16px 8px; font-size: 12px; font-weight: 500; color: var(--d-muted); display: flex; justify-content: space-between }
.mac .agc-think button { all: unset; cursor: pointer; display: flex; justify-content: space-between; align-items: center; min-height: 22px; width: calc(100% - 32px); margin: 0 6px; padding: 6px 10px; border-radius: 8px; font-size: 13px }
.mac .agc-think button.on { background: rgba(94,106,210,.12) }
.mac .agc-think .none { margin: 0 16px; font-size: 12px; color: var(--d-dim); line-height: 17px }
.mac .agc-menu { position: absolute; top: calc(100% + 4px); z-index: 8; min-width: 240px; padding: 4px 0; border: 1px solid var(--d-border); border-radius: 8px; background: #0f1011; box-shadow: 0 14px 40px rgba(0,0,0,.45) }
.mac .agc-menu button { all: unset; cursor: pointer; display: grid; grid-template-columns: 10px 1fr auto; gap: 8px; align-items: center; width: calc(100% - 24px); padding: 7px 12px; font-size: 13px; color: var(--d-fg) }
.mac .agc-menu button small { color: var(--d-muted); font-size: 11.5px }
.mac .agc-menu button.on { background: rgba(94,106,210,.12) }
.mac .agc-menu button[disabled] { color: var(--d-dim); cursor: default }
.mac .agc-menu .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--d-emerald) }
.mac .agc-menu .dot.off { background: var(--d-dim) }
.mac .agc-menu hr { border: 0; border-top: 1px solid var(--d-border); margin: 4px 0 }
/* Menus open upward when the room below is short, over everything, and scroll inside. */
.mac .agc-menu { max-height: 260px; overflow-y: auto; scrollbar-width: thin }
.mac .agc-menu.up { top: auto; bottom: calc(100% + 4px); z-index: 20 }
.mac .cc-view:has(.agc-menu, .agc-pick) + .cc-hints { visibility: hidden }
/* The machine menu's harness column lines up across rows. */
.mac .agc-menu.cols { display: grid; grid-template-columns: 10px auto minmax(0, 1fr) }
.mac .agc-menu.cols > button { grid-column: 1 / -1; display: grid; grid-template-columns: subgrid; width: auto }
.mac .agc-menu.cols > hr { grid-column: 1 / -1 }
.mac .agc-menu.cols small { text-align: left }
.mac .agconv { flex: 1 1 0; min-width: 0; min-height: 0; display: flex; flex-direction: column }
.mac .agctx { position: absolute; z-index: 5; min-width: 180px; padding: 4px; border-radius: var(--radius-sm); background: var(--elev, var(--d-elev)); border: 1px solid var(--d-border); box-shadow: 0 8px 24px color-mix(in srgb, #000 40%, transparent) }
.mac .agctx button { all: unset; display: block; box-sizing: border-box; width: 100%; cursor: pointer; padding: 5px 10px; border-radius: 5px; font-size: 12px; color: var(--d-fg) }
.mac .agctx button:hover, .mac .agctx button:focus { background: color-mix(in srgb, var(--d-fg) 8%, transparent) }
.mac .agctx hr { border: 0; border-top: 1px solid var(--d-border); margin: 4px 2px }
.mac .agcode { flex: 1; min-height: 0; overflow: auto }
.mac .agcode pre { margin: 0; padding: 8px 10px; font: 11.5px/17px var(--mono); color: var(--d-fg); white-space: pre; tab-size: 4 }
.mac .agcode .ln { display: inline-block; width: var(--lnw, 3ch); margin-right: 12px; text-align: right; color: var(--d-dim); -webkit-user-select: none; user-select: none }
.mac .agcode .k { color: #C792EA }
.mac .dcoderen .cmd .k { color: #C792EA } .mac .dcoderen .cmd .s { color: #A5D6A7 } .mac .dcoderen .cmd .c { color: var(--d-dim); font-style: italic } .mac .dcoderen .cmd .n { color: #F78C6C }
.mac .agcode .s { color: #A5D6A7 }
.mac .agcode .c { color: var(--d-dim); font-style: italic }
.mac .agcode .n { color: #F2B482 }
.mac #agmodel .agc-pill.agm { font-size: 12px; max-width: 260px; overflow: hidden; white-space: nowrap }
.mac #agmodel .agc-pill.agm[disabled] { opacity: .55; cursor: default }
.mac .agc-wrap { flex: 1; min-height: 0; overflow: auto; scrollbar-width: none; padding: 14px 14px 16px; display: flex; flex-direction: column }
.mac .agc { max-width: 760px; width: 100%; box-sizing: border-box; margin: auto auto 18px; display: grid; gap: 10px }
.mac .agc h3 { margin: 0 0 14px; text-align: center; font-size: 22px; font-weight: 500; letter-spacing: -.01em }
.mac .dbar[data-mode="window"] .agc h3 { font-size: 26px }
.mac .agc h3 b { font-weight: 600 }
.mac .agc-box { position: relative; border: 1px solid var(--d-border); border-radius: 16px; background: var(--d-surface); padding: 14px 14px 10px }
.mac .agc-box:focus-within { border-color: color-mix(in srgb, var(--d-emerald) 60%, transparent) }
.mac .agc-box.err { border-color: var(--d-err) }
.mac .agc-box textarea { display: block; width: 100%; min-height: 58px; max-height: 40vh; border: 0; outline: 0; background: transparent; color: var(--d-fg); font: inherit; font-size: 14.5px; line-height: 21px; resize: none; padding: 0; caret-color: var(--d-emerald); field-sizing: content }
.mac .agc-box textarea::placeholder { color: var(--d-dim) }
.mac .agc-box.err textarea::placeholder { color: var(--d-err) }
.mac .agc-row { display: flex; align-items: center; gap: 8px; margin-top: 6px }
.mac .agc-row .sp { flex: 1 }
.mac .agc-pill { all: unset; cursor: pointer; display: inline-flex; align-items: center; gap: 8px; padding: 5px 10px; border-radius: var(--radius-sm); font-size: 13px; color: var(--d-fg); min-width: 0; white-space: nowrap }
.mac .agc-pill:hover, .mac .agc-pill[aria-expanded="true"] { background: color-mix(in srgb, var(--d-fg) 6%, transparent) }
.mac .agc-pill:focus-visible { outline: 2px solid var(--d-emerald); outline-offset: 1px }
.mac .agc-pill[disabled] { color: var(--d-dim); cursor: default; background: transparent }
.mac .agc-pill small { color: var(--d-muted); font-size: 12.5px; overflow: hidden; text-overflow: ellipsis }
.mac .agc-pill svg { color: var(--d-muted); flex: none }
.mac .agc-pill .mono, .mac .agc-menu .mono, .mac .agc-list .mono { font-family: var(--mono); font-size: 12px }
.mac :is(.agc, #agmodel, #agpickm) .cc-ag { width: 22px; height: 22px; border-radius: var(--radius-sm); display: grid; place-items: center; font-size: 9.5px; font-weight: 600; border: 1px solid var(--d-border); background: var(--d-elev); flex: none }
.mac :is(.agc, #agmodel, #agpickm) .cc-ag.claude { color: #F2A27A }
.mac :is(.agc, #agmodel, #agpickm) .cc-ag.codex { color: var(--d-fg) }
.mac .agc-pill .cc-ag.codex svg { color: inherit }
.mac .agc-send { all: unset; cursor: pointer; width: 34px; height: 34px; border-radius: 50%; display: grid; place-items: center; background: var(--d-emerald); color: #fff; flex: none }
.mac .agc-send[disabled] { background: var(--d-elev); color: var(--d-dim); cursor: default }
.mac .agc-send:focus-visible { outline: 2px solid var(--d-fg); outline-offset: 2px }
.mac .agc-under { display: flex; align-items: center; flex-wrap: wrap; gap: 4px; padding: 0 6px; font-size: 12.5px; color: var(--d-muted); position: relative }
.mac .agc-under .agc-pill { font-size: 12.5px; color: var(--d-muted); padding: 4px 8px; max-width: 100% }
.mac .agc-under .agc-pill small { color: var(--d-dim); font-size: 11.5px }
.mac .agc-under .bar { width: 1px; height: 14px; background: var(--d-border) }
.mac .agc-type { position: relative; flex: 1; min-width: 200px }
.mac .agc-type input { width: 100%; box-sizing: border-box; padding: 5px 9px; border-radius: var(--radius-sm); border: 1px solid var(--d-emerald); background: var(--d-surface); color: var(--d-fg); font-family: var(--mono); font-size: 12.5px; outline: 0; caret-color: var(--d-emerald) }
.mac .agc-type .agc-menu { left: 0; right: 0 }
.mac .agc-type.base { flex: 0 1 220px; min-width: 160px }
.mac .agc-why { display: flex; gap: 8px; align-items: center; padding: 8px 12px; border-radius: var(--radius); border: 1px solid var(--d-border); font-size: 12.5px; color: var(--d-muted) }
.mac .agc-why[hidden] { display: none }
.mac .agc-why b { color: var(--d-fg); font-weight: 500 }
.mac .agc-note { display: flex; gap: 8px; align-items: center; padding: 0 6px; font-size: 12.5px; color: var(--d-muted) }
.mac .agc-help { border: 1px solid var(--d-border); border-radius: var(--radius); padding: 12px 14px; display: grid; gap: 8px; font-size: 13px }
.mac .agc-help ol { margin: 0; padding-left: 18px; color: var(--d-muted); display: grid; gap: 3px }
.mac .agc-help code { font-family: var(--mono); font-size: 12px; background: var(--d-elev); border: 1px solid var(--d-border); border-radius: 4px; padding: 0 4px }
.mac .agc-pick { position: fixed; top: 0; left: 0; z-index: 30; width: min(640px, calc(100vw - 16px)); box-sizing: border-box; display: grid; grid-template-columns: 58px minmax(0, 1fr) 184px; border: 1px solid var(--d-border); border-radius: var(--radius); background: color-mix(in srgb, var(--d-ink) 80%, #000); box-shadow: 0 18px 50px color-mix(in srgb, #000 50%, transparent); --pick-h: 340px; max-height: var(--pick-h); overflow: hidden auto; scrollbar-width: thin }
.mac .agc-rail { display: grid; align-content: start; gap: 8px; padding: 12px 11px; border-right: 1px solid var(--d-border); box-sizing: border-box; max-height: calc(var(--pick-h) - 2px); overflow-y: auto; scrollbar-width: none }
.mac .agc-rail button { all: unset; cursor: pointer; width: 34px; height: 34px; border-radius: var(--radius-sm); display: grid; place-items: center; position: relative }
.mac .agc-rail button[aria-pressed="true"] { background: color-mix(in srgb, var(--d-emerald) 14%, transparent) }
.mac .agc-rail button[aria-pressed="true"]::after { content: ""; position: absolute; right: -9px; top: 8px; bottom: 8px; width: 2px; border-radius: 2px; background: var(--d-emerald) }
.mac .agc-rail button[disabled] { opacity: .35; cursor: default }
.mac .agc-rail .cc-ag { width: 26px; height: 26px; font-size: 10px }
.mac .agc-list { display: grid; align-content: start; max-height: calc(var(--pick-h) - 2px); overflow-y: auto; scrollbar-width: thin; min-width: 0; padding-bottom: 8px; box-sizing: border-box }
.mac .agc-find { position: sticky; top: 0; z-index: 1; padding: 12px 12px 8px; background: color-mix(in srgb, var(--d-ink) 80%, #000) }
.mac .agc-models { display: grid; align-content: start; min-width: 0 }
.mac .agc-search { display: block; width: 100%; box-sizing: border-box; padding: 8px 12px; border: 1px solid var(--d-border); border-radius: var(--radius-sm); background: color-mix(in srgb, var(--d-fg) 4%, transparent); color: var(--d-fg); font: inherit; font-size: 13px; outline: 0; caret-color: var(--d-emerald) }
.mac .agc-search::placeholder { color: var(--d-dim) }
.mac .agc-search:focus { border-color: color-mix(in srgb, var(--d-emerald) 70%, transparent) }
.mac .agc-list button { all: unset; cursor: pointer; display: flex; align-items: center; gap: 8px; min-height: 22px; margin: 0 6px; padding: 6px 10px; border-radius: var(--radius-sm); font-size: 13px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .agc-list button:hover { background: color-mix(in srgb, var(--d-fg) 4%, transparent) }
.mac .agc-list button.on { background: color-mix(in srgb, var(--d-emerald) 14%, transparent) }
.mac .agc-list button small { color: var(--d-muted); font-size: 11.5px; margin-left: auto; overflow: hidden; text-overflow: ellipsis }
.mac .agc-list .none, .mac .agc-think .none { margin: 8px 16px; font-size: 12px; color: var(--d-dim); line-height: 17px; display: flex; gap: 6px; align-items: center }
.mac .agc-think { border-left: 1px solid var(--d-border); padding: 14px 0 10px; display: grid; align-content: start; box-sizing: border-box; max-height: calc(var(--pick-h) - 2px); overflow-y: auto; scrollbar-width: thin }
.mac .agc-think h5 { margin: 0 16px 8px; font-size: 12px; font-weight: 500; color: var(--d-muted); display: flex; justify-content: space-between; gap: 6px }
.mac .agc-think button { all: unset; cursor: pointer; display: flex; justify-content: space-between; align-items: center; min-height: 22px; margin: 0 6px; padding: 6px 10px; border-radius: var(--radius-sm); font-size: 13px }
.mac .agc-think button:hover { background: color-mix(in srgb, var(--d-fg) 4%, transparent) }
.mac .agc-think button.on { background: color-mix(in srgb, var(--d-emerald) 14%, transparent) }
.mac .agc-think button svg { color: var(--d-emerald) }
.mac .agc-menu { position: absolute; top: calc(100% + 4px); left: 0; z-index: 8; min-width: 240px; max-width: calc(100vw - 32px); max-height: 320px; overflow-y: auto; padding: 4px 0; border: 1px solid var(--d-border); border-radius: var(--radius); background: color-mix(in srgb, var(--d-ink) 80%, #000); box-shadow: 0 14px 40px color-mix(in srgb, #000 45%, transparent) }
.mac .agc-menu.base { left: auto; right: 0 }
.mac .agc-menu button { all: unset; box-sizing: border-box; cursor: pointer; display: grid; grid-template-columns: 10px minmax(0, 1fr) auto; gap: 8px; align-items: center; width: 100%; padding: 7px 12px; font-size: 13px; color: var(--d-fg) }
.mac .agc-menu button > span:nth-child(2) { overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .agc-menu button:hover { background: color-mix(in srgb, var(--d-fg) 4%, transparent) }
.mac .agc-menu button small { color: var(--d-muted); font-size: 11.5px }
.mac .agc-menu button.on { background: color-mix(in srgb, var(--d-emerald) 14%, transparent) }
.mac .agc-menu button[disabled] { color: var(--d-dim); cursor: default; background: transparent }
.mac .agc-menu .dot { width: 7px; height: 7px; border-radius: 50%; background: var(--d-emerald) }
.mac .agc-menu .dot.off { background: var(--d-dim) }
.mac .agc-menu hr { border: 0; border-top: 1px solid var(--d-border); margin: 4px 0 }
.mac .agc-menu.float { position: fixed; z-index: 40; right: auto; bottom: auto; overscroll-behavior: contain }
.mac .agc-menu.cols { display: grid; grid-template-columns: 10px auto minmax(0, 1fr); column-gap: 8px }
.mac .agc-menu.cols > button { grid-column: 1 / -1; grid-template-columns: subgrid }
.mac .agc-menu.cols > hr { grid-column: 1 / -1; width: 100% }
.mac .agc-menu.cols small { overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .agc-menuopen > .dhints { visibility: hidden }
.mac .agc h3 { font-size: 26px }
.mac .agc .cc-ag.mark { background-image: none }
.mac .agc-pill .cc-ag.mark { width: 22px; height: 22px; border-radius: var(--radius-sm); flex: none }
.mac .agc-under .sep { width: 1px; height: 14px; background: var(--d-border) }
.mac .agc-pick { position: absolute; top: auto; left: auto; right: 54px; bottom: 52px; width: min(640px, 100%); --pick-h: 340px }
.mac #agfield { position: relative }
.mac #agfield .agc-pick { left: 8px; right: auto; bottom: calc(100% + 6px); font-size: 13px; line-height: normal }
.mac .agc-box .pr.ph { color: var(--d-dim) }
.mac .agc-box.err { border-color: var(--d-err) }
.mac .agc-box.err .pr.ph { color: var(--d-err) }
.mac .agc-search { display: block }
.mac .agc-list .none, .mac .agc-think .none { margin: 8px 16px; font-size: 12px; color: var(--d-dim); line-height: 17px; display: flex; gap: 6px; align-items: center }
.mac .agc-under > .agc-menu.up { position: absolute }
.mac .ag-sess { display: flex; flex-direction: column; min-height: 0; flex: 1 }
.mac .ag-sess .dlist { flex: 1 1 auto; min-height: 0; overflow: auto; display: grid; align-content: start; gap: 2px; padding: 6px 8px 10px; scrollbar-width: none }
.mac .ag-sess .dchat .t { flex: 1; min-width: 0; display: grid }
.mac .ag-sess .dchat .t span { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: 14px }
.mac .ag-sess .dchat .t small { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-size: 12px; color: var(--d-muted) }
.mac .ag-sess .dchat .run { display: inline-flex; align-items: center; gap: 6px; font-size: 12px; color: var(--d-emerald); flex: none }
.mac .dchat:hover { background: rgba(227,228,230,.04) }
.mac .dstate { font-size: 11px; font-weight: 600; padding: 0 8px; line-height: 19px; border-radius: 999px; border: 1px solid var(--d-border); color: var(--d-muted); flex: none; white-space: nowrap }
.mac .dstate.ask { border-color: color-mix(in srgb, #f2994a 50%, transparent); color: #f2994a }`;
  document.head.append(st);

  const AGK = { claude: ["CC", "Claude Code"], codex: ["CX", "Codex"], gemini: ["GE", "Gemini CLI"], pi: ["PI", "pi"], flsh: ["FL", "flsh"], opencode: ["OC", "OpenCode"], hermes: ["HE", "Hermes"] };
  const MARKS = {
  claude: '<svg aria-hidden="true" viewBox="0 0 24 24"><path fill="#D97757" d="m4.7144 15.9555 4.7174-2.6471.079-.2307-.079-.1275h-.2307l-.7893-.0486-2.6956-.0729-2.3375-.0971-2.2646-.1214-.5707-.1215-.5343-.7042.0546-.3522.4797-.3218.686.0608 1.5179.1032 2.2767.1578 1.6514.0972 2.4468.255h.3886l.0546-.1579-.1336-.0971-.1032-.0972L6.973 9.8356l-2.55-1.6879-1.3356-.9714-.7225-.4918-.3643-.4614-.1578-1.0078.6557-.7225.8803.0607.2246.0607.8925.686 1.9064 1.4754 2.4893 1.8336.3643.3035.1457-.1032.0182-.0728-.164-.2733-1.3539-2.4467-1.445-2.4893-.6435-1.032-.17-.6194c-.0607-.255-.1032-.4674-.1032-.7285L6.287.1335 6.6997 0l.9957.1336.419.3642.6192 1.4147 1.0018 2.2282 1.5543 3.0296.4553.8985.2429.8318.091.255h.1579v-.1457l.1275-1.706.2368-2.0947.2307-2.6957.0789-.7589.3764-.9107.7468-.4918.5828.2793.4797.686-.0668.4433-.2853 1.8517-.5586 2.9021-.3643 1.9429h.2125l.2429-.2429.9835-1.3053 1.6514-2.0643.7286-.8196.85-.9046.5464-.4311h1.0321l.759 1.1293-.34 1.1657-1.0625 1.3478-.8804 1.1414-1.2628 1.7-.7893 1.36.0729.1093.1882-.0183 2.8535-.607 1.5421-.2794 1.8396-.3157.8318.3886.091.3946-.3278.8075-1.967.4857-2.3072.4614-3.4364.8136-.0425.0304.0486.0607 1.5482.1457.6618.0364h1.621l3.0175.2247.7892.522.4736.6376-.079.4857-1.2142.6193-1.6393-.3886-3.825-.9107-1.3113-.3279h-.1822v.1093l1.0929 1.0686 2.0035 1.8092 2.5075 2.3314.1275.5768-.3218.4554-.34-.0486-2.2039-1.6575-.85-.7468-1.9246-1.621h-.1275v.17l.4432.6496 2.3436 3.5214.1214 1.0807-.17.3521-.6071.2125-.6679-.1214-1.3721-1.9246L14.38 17.959l-1.1414-1.9428-.1397.079-.674 7.2552-.3156.3703-.7286.2793-.6071-.4614-.3218-.7468.3218-1.4753.3886-1.9246.3157-1.53.2853-1.9004.17-.6314-.0121-.0425-.1397.0182-1.4328 1.9672-2.1796 2.9446-1.7243 1.8456-.4128.164-.7164-.3704.0667-.6618.4008-.5889 2.386-3.0357 1.4389-1.882.929-1.0868-.0062-.1579h-.0546l-6.3385 4.1164-1.1293.1457-.4857-.4554.0608-.7467.2307-.2429 1.9064-1.3114Z"/></svg>',
  codex: '<svg aria-hidden="true" viewBox="0 0 24 24"><path fill="currentColor" d="M22.2819 9.8211a5.9847 5.9847 0 0 0-.5157-4.9108 6.0462 6.0462 0 0 0-6.5098-2.9A6.0651 6.0651 0 0 0 4.9807 4.1818a5.9847 5.9847 0 0 0-3.9977 2.9 6.0462 6.0462 0 0 0 .7427 7.0966 5.98 5.98 0 0 0 .511 4.9107 6.051 6.051 0 0 0 6.5146 2.9001A5.9847 5.9847 0 0 0 13.2599 24a6.0557 6.0557 0 0 0 5.7718-4.2058 5.9894 5.9894 0 0 0 3.9977-2.9001 6.0557 6.0557 0 0 0-.7475-7.0729zm-9.022 12.6081a4.4755 4.4755 0 0 1-2.8764-1.0408l.1419-.0804 4.7783-2.7582a.7948.7948 0 0 0 .3927-.6813v-6.7369l2.02 1.1686a.071.071 0 0 1 .038.052v5.5826a4.504 4.504 0 0 1-4.4945 4.4944zm-9.6607-4.1254a4.4708 4.4708 0 0 1-.5346-3.0137l.142.0852 4.783 2.7582a.7712.7712 0 0 0 .7806 0l5.8428-3.3685v2.3324a.0804.0804 0 0 1-.0332.0615L9.74 19.9502a4.4992 4.4992 0 0 1-6.1408-1.6464zM2.3408 7.8956a4.485 4.485 0 0 1 2.3655-1.9728V11.6a.7664.7664 0 0 0 .3879.6765l5.8144 3.3543-2.0201 1.1685a.0757.0757 0 0 1-.071 0l-4.8303-2.7865A4.504 4.504 0 0 1 2.3408 7.872zm16.5963 3.8558L13.1038 8.364 15.1192 7.2a.0757.0757 0 0 1 .071 0l4.8303 2.7913a4.4944 4.4944 0 0 1-.6765 8.1042v-5.6772a.79.79 0 0 0-.407-.667zm2.0107-3.0231l-.142-.0852-4.7735-2.7818a.7759.7759 0 0 0-.7854 0L9.409 9.2297V6.8974a.0662.0662 0 0 1 .0284-.0615l4.8303-2.7866a4.4992 4.4992 0 0 1 6.6802 4.66zM8.3065 12.863l-2.02-1.1638a.0804.0804 0 0 1-.038-.0567V6.0742a4.4992 4.4992 0 0 1 7.3757-3.4537l-.142.0805L8.704 5.459a.7948.7948 0 0 0-.3927.6813zm1.0976-2.3654l2.602-1.4998 2.6069 1.4998v2.9994l-2.5974 1.4997-2.6067-1.4997Z"/></svg>',
  gemini: '<svg aria-hidden="true" viewBox="0 0 24 24"><path fill="#8E9CF5" d="M11.04 19.32Q12 21.51 12 24q0-2.49.93-4.68.96-2.19 2.58-3.81t3.81-2.55Q21.51 12 24 12q-2.49 0-4.68-.93a12.3 12.3 0 0 1-3.81-2.58 12.3 12.3 0 0 1-2.58-3.81Q12 2.49 12 0q0 2.49-.96 4.68-.93 2.19-2.55 3.81a12.3 12.3 0 0 1-3.81 2.58Q2.49 12 0 12q2.49 0 4.68.96 2.19.93 3.81 2.55t2.55 3.81"/></svg>',
  pi: '<svg aria-hidden="true" viewBox="0 0 24 24"><path fill="currentColor" fill-rule="evenodd" d="M3 2h13v3h3v8h-3v3H9v6H3zM9 7v4h5V7zM17 18h4v4h-4z"/></svg>',
  flsh: '<svg aria-hidden="true" viewBox="0 0 28 28"><path fill="currentColor" d="M14 1 27 27h-3.1L15.4 10v17h-2.8V10L4.1 27H1Z"/></svg>',
  opencode: '<svg aria-hidden="true" viewBox="96 96 320 320"><path fill="#5A5858" d="M320 224v128H192V224z"/><path fill="#F1ECEC" fill-rule="evenodd" d="M384 416H128V96h256zM320 160H192v192h128z"/></svg>',
};
  window.__MARKS = MARKS;
  const av = a => `<span class="cc-ag ${a}${MARKS[a] ? " mark" : ""}">${MARKS[a] || AGK[a][0]}</span>`;
  const caret = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>';
  const up = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M5 12l7-7 7 7"/></svg>';
  const clip = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.5l-8.5 8.5a5 5 0 01-7-7L14 5.5a3.3 3.3 0 014.7 4.7L10.2 18.7a1.7 1.7 0 01-2.4-2.4L15.5 8.6"/></svg>';
  const host = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="12" rx="2"/><path d="M8 20h8M12 16v4"/></svg>';
  const folder = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2z"/></svg>';
  const MACHINES = [["build-box", "Claude Code, Codex, pi, flsh, Hermes", true], ["mac-box", "Claude Code, Codex, Gemini CLI", true], ["win-box", "", false]];
  const MODELS = { hermes: [["Default", "its own"], ["openrouter:vendor/model-a", ""]], claude: [["Default", "model-large"], ["model-large", ""], ["model-small", ""]],
    codex: [["Default", "gpt-test"], ["gpt-test", ""], ["gpt-mini", ""]],
    flsh: [["Default", "openai:gpt-test"], ["anthropic:model-large", ""], ["openrouter:vendor/model-a", ""], ["ollama:local-model", ""]] };
  const THINK = { claude: ["Low", "Medium", "High", "Extra high", "Max"], codex: ["Low", "Medium", "High"], flsh: ["Off", "Low", "Medium", "High"] };

  const pick = (key, ok, def) => ok.includes(S.d[key]) ? S.d[key] : def;
  const CHECK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12l5 5 9-10"/></svg>';
  const BRANCH = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="7" r="2"/><path d="M6 7v10M18 9c0 5-6 4-11 8"/></svg>';
  // The same picker for the start form and a session's pill (agentpicker.ts).
  const agcPicker = ({ agent, offered, machine, models = true, act = "" }) => {
    const name = AGK[agent][1], levels = THINK[agent];
    const rail = ["claude", "codex", "gemini", "pi", "flsh", "hermes"].map(a => { const on = offered.includes(a);
      return `<button type="button" aria-pressed="${a === agent}" title="${AGK[a][1]}${on ? "" : ` (not on ${machine || "any machine"})`}"${on ? "" : " disabled"}>${av(a)}</button>`; }).join("");
    const think = levels
      ? [`<button type="button" class="on">Default${CHECK}</button>`, ...levels.map(t => `<button type="button">${t}</button>`)].join("")
      : `<p class="none">${name} has no thinking setting Beep can pass.</p>`;
    const rows = !models ? `<button type="button" class="on">Default<small>its own</small></button><p class="none">Pick a machine to list its models.</p>`
      : MODELS[agent].map(([m, note], i) => `<button type="button" class="${i === 0 ? "on" : ""}${agent === "flsh" && i ? " mono" : ""}"${act ? ` data-dact="${act}"` : ""}>${m}${note ? `<small>${note}</small>` : ""}</button>`).join("");
    return `<div class="agc-pick" role="dialog" aria-label="Harness, model and thinking">
        <div class="agc-rail">${rail}</div>
        <div class="agc-list"><div class="agc-find"><input class="agc-search" spellcheck="false" autocomplete="off" placeholder="Search ${name} models…"></div><div class="agc-models">${rows}</div></div>
        <div class="agc-think"><h5><span>Thinking</span><span>${name}</span></h5>${think}</div>
      </div>`;
  };
  window.__agcPicker = agcPicker;
  const AGV = ["ready", "nohost", "picker", "flsh", "hermes", "pickernomachine", "host", "project", "folder", "branch", "basemenu", "typing", "none", "looking", "noprompt", "noproj", "noprojfolder", "code"];
  D.dAgentNew = () => {
    const v = pick("agc", AGV, "nohost");
    const none = v === "none", looking = v === "looking", noprompt = v === "noprompt";
    // Started from Home or the sidebar, outside a project: the project pill reads No project, and the
    // folder menu lists the projects checked out on the machine.
    const np = v === "noproj" || v === "noprojfolder" || none || looking;
    const h = v === "hermes" ? "hermes" : v === "flsh" ? "flsh" : "claude";
    const chosenHost = ["nohost", "host", "pickernomachine", "project", "none", "looking"].includes(v) ? null : "build-box";
    const pillText = none ? "No agent" : `${av(h)}${AGK[h][1]} <small>Default</small>`;
    const open = v === "picker" || v === "flsh" || v === "hermes" || v === "pickernomachine";
    const picker = open ? agcPicker({ agent: h, offered: ["claude", "codex", "pi", "flsh", "hermes"], machine: chosenHost, models: v !== "pickernomachine" }) : "";
    // Every menu under the box opens upward over the page; the key hints hide while one is open.
    const hostMenu = v === "host" ? `<div class="agc-menu cols up" style="left:90px" role="menu">
        ${MACHINES.map(([n, has, on]) => `<button${on ? "" : " disabled"}><span class="dot${on ? "" : " off"}"></span><span>${n}</span><small>${on ? has : "offline"}</small></button>`).join("")}</div>` : "";
    const projectMenu = v === "project" ? `<div class="agc-menu up" role="menu">
        <button class="on"><span></span><span>acme/widget</span><small>2 machines</small></button>
        <button><span></span><span>acme/relay</span><small>1 machine</small></button><hr>
        <button><span></span>No project<small>pick one to start</small></button></div>` : "";
    const folderMenu = v === "noprojfolder" ? `<div class="agc-menu folder up" style="left:210px" role="menu">
        <button class="on"><span></span>Workspace<small class="mono">~</small></button><hr>
        <button><span></span><span>acme/widget</span><small class="mono">~/acme/widget</small></button>
        <button><span></span><span>acme/relay</span><small class="mono">~/srv/relay</small></button><hr>
        <button><span></span>Type a folder…<small></small></button></div>` : v === "folder" ? `<div class="agc-menu folder up" style="left:210px" role="menu">
        <button class="on"><span></span><span class="mono">~/acme/widget</span><small>checkout</small></button><hr>
        <button class="on"><span></span>Current checkout<small>as it is</small></button>
        <button><span></span>New branch<small>in the checkout, named from the prompt</small></button>
        <button><span></span>New worktree<small>new branch in .worktrees/</small></button>
        <button><span></span><span class="mono">beep/ios-approvals</span><small>worktree</small></button><hr>
        <button><span></span>Workspace<small class="mono">~</small></button>
        <button><span></span>Type a folder…<small></small></button></div>` : "";
    const baseMenu = v === "basemenu" ? `<div class="agc-menu base up" role="menu">
        <button class="on"><span></span><span class="mono">main</span><small>default</small></button>
        <button><span></span><span class="mono">release/1.2</span><small></small></button>
        <button><span></span><span class="mono">fix-login</span><small></small></button><hr>
        <button><span></span>Type a branch or commit…<small></small></button></div>` : "";
    const makes = v === "branch" || v === "basemenu";
    const base = makes ? `<span class="sep"></span><button class="agc-pill" aria-expanded="${v === "basemenu"}" title="The branch the new one starts from" data-dact="dset:agc:${v === "basemenu" ? "branch" : "basemenu"}">${BRANCH}<small>from</small> <span class="mono">main</span>${caret}</button>` : "";
    // No Auto: the user picks the machine, remembered per project. Start waits for one.
    const why = chosenHost || none || looking ? "" : `<div class="agc-why"><span class="ddot off"></span><span>Pick a machine to start.</span></div>`;
    const help = looking ? `<div class="agc-note"><span class="dspin"></span> Looking for machines…</div>` : none ? `<div class="agc-help"><b>No machine can run an agent yet</b>
        <ol><li>Install beep-node on a computer that has Claude Code, Codex, Gemini CLI, pi or flsh.</li><li>Run <code>beep-node link</code> there.</li><li>Paste the link it prints into a chat here.</li></ol>
        <div><button class="dbtn" data-dgo="dMachines">Machines settings</button></div></div>` : "";
    const prompt = none || looking || noprompt ? "" : "Fix the failing login test";
    const pjIc = '<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="7" height="7" rx="1.5"/><rect x="14" y="3" width="7" height="7" rx="1.5"/><rect x="3" y="14" width="7" height="7" rx="1.5"/><rect x="14" y="14" width="7" height="7" rx="1.5"/></svg>';
    const pjPill = `<button class="agc-pill" aria-expanded="${v === "project"}" title="${np ? "Pick a project to start" : "The project this agent works on"}" data-dact="dset:agc:${v === "project" ? "nohost" : "project"}">${pjIc}${np ? "No project" : "acme/widget"}${caret}</button><span class="sep"></span>`;
    const pathText = "~/acme/widget";
    const folderPill = v === "typing"
      ? `<span class="agc-type"><input spellcheck="false" autocomplete="off" role="combobox" aria-label="Folder" placeholder="/path/to/folder or ~/folder" value="~/acme/"><span><div class="agc-menu up" role="listbox"><button class="on" role="option"><span></span><span class="mono">relay</span><small></small></button><button role="option"><span></span><span class="mono">widget</span><small></small></button></div></span></span>`
      : `<button class="agc-pill" aria-expanded="${v === "folder" || v === "noprojfolder"}" title="${np ? "The machine's workspace" : pathText}" data-dact="dset:agc:${np ? (v === "noprojfolder" ? "noproj" : "noprojfolder") : v === "folder" ? "nohost" : "folder"}">${folder}${np ? 'Workspace <small class="mono">~</small>' : `<span class="mono">${pathText}</span><small>${makes ? "New worktree" : "Current checkout"}</small>`}${caret}</button>`;
    const form = `<div class="agc"><h3>${np ? "What should we build?" : "What should we build in <b>widget</b>?"}</h3>
      <div class="agc-box${noprompt ? " err" : ""}"><div class="pr${prompt ? "" : " ph"}">${v === "code"
        // Text between two triple line breaks goes out in a code fence; a long
        // paste in it is a pill that expands to the full text first.
        ? `Why does the login test fail?<span class="cx-fence"><span class="cx-chip" data-toast="Shows the 42 lines; Expand puts them in the prompt">Pasted text <small>· 42 lines</small></span><br>cargo test -p widget-core</span>Keep the error text as it is.`
        : prompt || (noprompt ? "Say what the agent should do." : "Describe the task.")}</div>
        <div class="agc-row"><span class="sp"></span>
          ${none || looking ? `<button class="agc-pill" disabled>No agent</button>` : `<button class="agc-pill" aria-expanded="${open}" title="Harness, model and thinking (⌘M)" data-dact="dset:agc:${v === "picker" ? "nohost" : "picker"}">${pillText}${caret}</button>`}
          <button class="dbtn quiet" type="button" aria-label="Attach files" title="Attach files" data-toast="Choose files"><svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><path d="M12 5v14M5 12h14"/></svg></button><button class="agc-send"${np ? ' disabled title="Pick a project first"' : none || looking || !chosenHost ? ' disabled title="Pick a machine first"' : ` data-dgo="dAgentSession" data-dact="dset:ags:working" title="Enter starts, Shift+Enter adds a line"`} aria-label="Start">${up}</button></div>${picker}</div>
      <div class="agc-under">${pjPill}<button class="agc-pill" aria-expanded="${v === "host"}" data-dact="dset:agc:${v === "host" ? "nohost" : "host"}">${host}${chosenHost || "Pick a machine"}${caret}</button><span class="sep"></span>
        ${folderPill}${base}${projectMenu}${hostMenu}${folderMenu}${baseMenu}</div>
      ${why}${help}</div>`;
    return ccWin(ccSidePj("agents"), `<div class="cc-top"><h2>Start a coding agent</h2><small>It runs on your machine; follow it and answer it here</small><span class="tools"><button class="dbtn small" data-dgo="dAgents">All sessions</button></span></div>
      <div class="cc-view">${form}</div>
      <div class="cc-hints"><span><kbd>${DI.ret}</kbd>start</span><span><kbd>⌘M</kbd>model</span><span><kbd>esc</kbd>cancel</span></div>`);
  };
  const vr = (l, k) => [l, () => { S.d.agc = k; render(); }, () => pick("agc", AGV, "nohost") === k];
  VARIANTS.dAgentNew = [vr("No machine picked yet", "nohost"), vr("Picker open (Claude Code)", "picker"), vr("Picker open (flsh)", "flsh"), vr("Picker open (Hermes)", "hermes"), vr("Picker, no machine", "pickernomachine"), vr("Machine menu", "host"), vr("Project menu", "project"), vr("Folder menu", "folder"), vr("New worktree from a ref", "branch"), vr("Base menu", "basemenu"), vr("Typing a folder", "typing"), vr("No agents", "none"), vr("Looking for machines", "looking"), vr("No prompt", "noprompt"), vr("No project", "noproj"), vr("No project: folder menu", "noprojfolder"), vr("Pasted text sent as code", "code")];
})();
