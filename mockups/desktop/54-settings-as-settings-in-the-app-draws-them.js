// ---- Settings, as settings/ in the app draws them ----
// One set of sections: the window's sheet shows one page at a time (settingsSections with
// sheet=true), the bar shows them in a column. Apps, an app, an MCP server, Agent accounts,
// Credentials, Instructions and a coding agent are the same markup in the sheet, the bar and
// the window's own views.
const SETV = (() => {
  const st = document.createElement("style");
  st.textContent = `.mac .cc-set-scrim { position: fixed; inset: 0; z-index: 30; background: color-mix(in srgb, #000 45%, transparent); display: flex; align-items: center; justify-content: center }
.mac .cc-set { width: min(1040px, 92%); height: min(720px, 88%); display: grid; grid-template-columns: 218px minmax(0, 1fr); border-radius: var(--radius); border: 1px solid var(--d-border); background: var(--d-surface); box-shadow: 0 20px 60px color-mix(in srgb, #000 50%, transparent); overflow: hidden; outline: 0 }
.mac .cc-set .dset-nav { padding: 14px 8px; border-right: 1px solid var(--d-border); display: flex; flex-direction: column; gap: 2px; overflow-y: auto; background: color-mix(in srgb, var(--d-fg) 1.2%, transparent); scrollbar-width: thin }
.mac .cc-set .dset-nav > b { padding: 0 10px; font-size: 15px; font-weight: 600 }
.mac .cc-set .dset-nav > b { margin-bottom: 10px }
.mac .cc-set .dset-nav > small.dset-ver { margin-top: auto; padding: 10px 10px 0; font-size: 11.5px; color: var(--d-muted) }
.mac .cc-set .dset-pages { display: flex; flex-direction: column; gap: 2px }
.mac .cc-set .dset-pages .cc-si { padding: 6px 10px }
.mac .cc-set .dset-pages .cc-si.sub { padding: 4px 10px 4px 24px; font-size: 12.5px; color: var(--d-muted) }
.mac .cc-set .dset-pages .cc-si.sub[aria-current="true"] { color: var(--d-fg) }
.mac .cc-set .dset-body { display: flex; flex-direction: column; min-width: 0; min-height: 0 }
.mac .cc-set .dset-body > .cc-skel { margin: 14px 24px }
.mac .cc-set .dset-top { flex: none; display: flex; align-items: center; gap: 10px; padding: 14px 24px 12px; border-bottom: 1px solid var(--d-border) }
.mac .cc-set .dset-top .tt { display: grid }
.mac .cc-set .dset-top b { font-size: 15px; font-weight: 600 }
.mac .cc-set .dset-top small { font-size: 12px; color: var(--d-muted) }
.mac .cc-set .dset-top kbd { font: inherit; font-size: 11px; opacity: .7 }
.mac .cc-set .dset-crumbs { display: flex; align-items: center; gap: 6px; min-width: 0; white-space: nowrap }
.mac .cc-set .dset-crumbs > svg { flex: none; color: var(--d-dim) }
.mac .cc-set .dset-crumb { all: unset; cursor: pointer; font-size: 15px; color: var(--d-muted) }
.mac .cc-set .dset-crumb:hover, .mac .cc-set .dset-crumb:focus-visible { color: var(--d-fg) }
.mac .cc-set .dset-back { all: unset; cursor: pointer; flex: none; display: inline-grid; place-items: center; width: 24px; height: 24px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); color: var(--d-muted) }
.mac .cc-set .dset-back:hover, .mac .cc-set .dset-back:focus-visible { color: var(--d-fg) }
.mac .cc-set .dset-top kbd { margin-left: auto; flex: none }
.mac .cc-set .dset-crumbs b { overflow: hidden; text-overflow: ellipsis }
.mac .cc-set .dset-page { flex: 1; min-height: 0; overflow: auto; padding: 16px 24px 24px; scrollbar-width: thin; display: flex; flex-direction: column }
.mac .cc-set .dset-page > section { max-width: 780px }
.mac .cc-set .dset-page > section > .dsec.flat:first-child { display: none }
.mac .cc-set .dset-host > .cc-view { padding: 0; overflow: visible }
.mac .cc-set .dset-host > .dhead { padding: 0 0 12px; border-bottom: 0 }
.mac .cc-set .dset-host > .dlist { padding: 0; overflow: visible }
.mac .cc-set .dset-h { font-size: 12px; font-weight: 600; letter-spacing: 0; text-transform: none; color: var(--d-dim); margin: 16px 0 6px }
.mac .cc-set .dset-box { border: 1px solid var(--d-border); border-radius: var(--radius); overflow: hidden; margin: 0 0 14px }
.mac .cc-set .dset-row { display: grid; grid-template-columns: 28px minmax(0, 1fr) auto; gap: 12px; align-items: center; padding: 10px 14px; border-bottom: 1px solid var(--d-border); cursor: pointer; outline: none }
.mac .cc-set .dset-row:last-child { border-bottom: 0 }
.mac .cc-set .dset-row:hover { background: color-mix(in srgb, var(--d-fg) 2.5%, transparent) }
.mac .cc-set .dset-row:focus-visible { box-shadow: inset 0 0 0 2px var(--d-emerald) }
.mac .cc-set .dset-row .tt { display: grid; min-width: 0 }
.mac .cc-set .dset-row b { font-weight: 500; font-size: 13.5px }
.mac .cc-set .dset-row small { font-size: 12px; color: var(--d-muted); line-height: 17px }
.mac .cc-set .dset-row > svg { color: var(--d-muted) }
.mac .cc-set .dset-box .dmach { padding: 10px 14px }
.mac .cc-set .dmach[data-machine] { cursor: pointer; outline: none }
.mac .cc-set .dmach[data-machine]:hover { background: color-mix(in srgb, var(--d-fg) 2.5%, transparent) }
.mac .cc-set .dmach[data-machine]:focus-visible { box-shadow: inset 0 0 0 2px var(--d-emerald) }
.mac .cc-set .dmach[data-machine] .nm > svg { margin-left: auto; color: var(--d-muted) }
.mac .cc-set .dset-kv { display: grid; grid-template-columns: 170px minmax(0, 1fr); font-size: 13px }
.mac .cc-set .dset-kv > span { padding: 8px 14px; border-bottom: 1px solid var(--d-border); min-width: 0; overflow-wrap: anywhere }
.mac .cc-set .dset-kv > span:nth-child(odd) { color: var(--d-muted) }
.mac .cc-set .dset-kv > span:nth-last-child(-n+2) { border-bottom: 0 }
.mac .cc-set .dset-tbl { display: table; width: 100%; border-collapse: collapse; font-size: 13px }
.mac .cc-set .dset-tbl th { text-align: left; font-weight: 500; font-size: 11.5px; color: var(--d-muted); padding: 8px 14px; border-bottom: 1px solid var(--d-border) }
.mac .cc-set .dset-tbl td { padding: 7px 14px; border-bottom: 1px solid var(--d-border) }
.mac .cc-set .dset-tbl tr:last-child td { border-bottom: 0 }
.mac .cc-set .dset-rm { padding: 0 }
.mac .dset-rm-row { display: flex; align-items: center; gap: 12px; margin-top: 20px; padding-top: 14px; border-top: 1px solid var(--d-border) }
.mac .dset-rm-row .t { flex: 1; min-width: 0; display: grid; gap: 2px }
.mac .dset-rm-row b { font-size: 13px; font-weight: 500 }
.mac .dset-rm-row small { font-size: 12px; color: var(--d-muted) }
.mac .agproj { padding: 0; border: 0; background: none; color: var(--d-fg); font: inherit; cursor: pointer }
.mac .cc-set section { scroll-margin-top: 4px }
.mac .in-view { display: grid; gap: 14px; align-content: start }
.mac .in-tabs { display: flex; gap: 6px; flex-wrap: wrap }
.mac .in-tabs .tm-tab { padding: 5px 11px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: transparent; color: var(--d-muted); font: inherit; font-size: 12.5px }
.mac .in-tabs .tm-tab[aria-current="true"] { background: color-mix(in srgb, var(--d-fg) 8%, transparent); color: var(--d-fg) }
.mac .in-cols { display: grid; grid-template-columns: minmax(0, 1.6fr) minmax(220px, 1fr); gap: 14px; align-items: start }
.mac .in-ed { display: grid; gap: 10px; padding: 10px 12px }
.mac .in-ver { display: flex; justify-content: space-between; font-size: 12px; color: var(--d-muted) }
.mac .in-ver b { color: var(--d-fg); font-weight: 600 }
.mac .in-ed textarea { min-height: 300px; resize: vertical; padding: 10px 12px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: color-mix(in srgb, #000 25%, transparent); color: var(--d-fg); font: 12.5px/1.55 var(--mono, ui-monospace, monospace); outline: 0; user-select: text }
.mac .in-ed textarea:focus { border-color: color-mix(in srgb, var(--d-emerald) 60%, transparent) }
.mac .in-acts { display: flex; align-items: center; gap: 6px; flex-wrap: wrap }
.mac .in-acts small { margin-left: auto; color: var(--d-muted); font-size: 11.5px }
.mac .in-side { grid-template-columns: minmax(0, 1fr) auto }
.mac .in-sidecol { display: grid; gap: 14px }
.mac .in-layers { margin: 0; padding: 6px 0; list-style: none; counter-reset: l }
.mac .in-layers li { counter-increment: l; display: grid; grid-template-columns: 20px minmax(0, 1fr); gap: 0 8px; padding: 6px 10px; font-size: 12.5px; line-height: 17px }
.mac .in-layers li::before { content: counter(l); font-size: 11px; color: var(--d-dim); text-align: right; font-variant-numeric: tabular-nums }
.mac .in-layers li small { grid-column: 2; font-size: 11.5px; color: var(--d-muted) }
.mac .in-layers li.me { background: color-mix(in srgb, var(--d-emerald) 14%, transparent) }
.mac .in-layers b { font-weight: 500; color: var(--d-fg) }
.mac .in-mach { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 6px 10px; padding: 10px 12px; border-bottom: 1px solid var(--d-border) }
.mac .in-mach:last-child { border-bottom: 0 }
.mac .in-mach > b { display: flex; align-items: center; gap: 8px; font-size: 13px; font-weight: 600 }
.mac .in-files { grid-column: 1 / -1; display: grid; gap: 2px }
.mac .in-file { display: grid; grid-template-columns: 96px minmax(0, 1fr) auto; align-items: center; gap: 10px; padding: 4px 0; font-size: 12.5px }
.mac .in-file.miss { color: var(--d-muted) }
.mac .in-file .ag { color: var(--d-muted) }
.mac .in-file code { font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .in-file .r { display: flex; align-items: center; gap: 6px; color: var(--d-muted) }
.mac .in-ok { color: var(--d-emerald) }
.mac .in-warn { color: #f2c94c }
.mac .in-pre { margin: 4px 0 6px; padding: 8px 12px; border-radius: var(--radius-sm); background: color-mix(in srgb, #000 25%, transparent); font-size: 11.5px; line-height: 1.5; white-space: pre-wrap; max-height: 260px; overflow: auto; user-select: text }
.mac .in-hist { padding: 10px 12px; border-bottom: 1px solid var(--d-border) }
.mac .in-hist:last-child { border-bottom: 0 }
.mac .in-hist .h { display: flex; align-items: center; gap: 10px; font-size: 12.5px }
.mac .in-hist .h small { color: var(--d-muted) }
.mac .in-hist .acts { margin-left: auto; display: flex; gap: 6px }
.mac .hv-view { display: grid; gap: 12px; align-content: start }
.mac .hv { display: grid; grid-template-columns: 300px minmax(0, 1fr); border: 1px solid var(--d-border); border-radius: var(--radius); overflow: hidden; min-height: 0 }
.mac .hv-list { border-right: 1px solid var(--d-border); display: grid; align-content: start; background: color-mix(in srgb, var(--d-fg) 1.5%, transparent) }
.mac .hv-it { cursor: pointer; display: grid; grid-template-columns: 28px minmax(0, 1fr) 36px; gap: 10px; align-items: center; padding: 10px 14px; border-bottom: 1px solid var(--d-border); outline: none }
.mac .hv-it:focus-visible { box-shadow: inset 0 0 0 2px var(--d-emerald) }
.mac .hv-it[aria-current="true"] { background: color-mix(in srgb, #4cb782 7%, transparent) }
.mac .hv-it .dswitch, .mac .hv-sum .dswitch { justify-self: end }
.mac .hv-it b { font-weight: 500; font-size: 13.5px }
.mac .hv-it.off b, .mac .hv-it.off .cc-ag, .mac .hv-sum.off b, .mac .hv-sum.off .cc-ag { opacity: .45 }
.mac .hv-name { min-width: 0 }
.mac .hv-name small { display: block; margin-top: 2px; font-size: 12px; line-height: 17px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .hv-name small.bad { color: #f2994a }
.mac .hv-nm { display: flex; align-items: baseline; gap: 6px; min-width: 0; white-space: nowrap }
.mac .hv-nm b { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis }
.mac .hv-nm .ver { flex: 0 1000 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; font-family: var(--mono); font-size: 11.5px; color: var(--d-muted) }
.mac .hv-up { flex: none; align-self: center; font-size: 10.5px; line-height: 15px; padding: 0 6px; border-radius: 999px; color: #f2c94c; border: 1px solid color-mix(in srgb, #f2c94c 45%, transparent); background: color-mix(in srgb, #f2c94c 8%, transparent) }
.mac .hv-d { padding: 16px 20px; overflow-y: auto; scrollbar-width: thin; min-width: 0;  }
.mac .hv-d h3 { margin: 0; display: flex; align-items: center; gap: 10px; font-size: 17px; font-weight: 600 }
.mac .hv-d h3 .ver { margin-left: auto; font-family: var(--mono); font-size: 12px; color: var(--d-muted); font-weight: 400 }
.mac .hv-d h4 { margin: 20px 0 4px; font-size: 13.5px; font-weight: 600 }
.mac .hv-f { display: grid; grid-template-columns: 200px minmax(0, 1fr); gap: 20px; align-items: start; padding: 11px 0; border-bottom: 1px solid var(--d-border) }
.mac .hv-f > :last-child { min-width: 0; align-self: center }
.mac .hv-f b { font-weight: 500; font-size: 13px; overflow-wrap: anywhere }
.mac .hv-f small { display: block; margin-top: 2px; font-size: 12px; line-height: 17px; color: var(--d-muted) }
.mac .hv-in { height: 32px; padding: 0 11px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: var(--d-ink); font-family: var(--mono); font-size: 12.5px; color: var(--d-fg); min-width: 0; width: 100%; box-sizing: border-box; user-select: text }
.mac .hv-in::placeholder { color: var(--d-dim) }
.mac .hv-envs { display: grid; gap: 6px }
.mac .hv-env { display: grid; grid-template-columns: minmax(0, 2fr) minmax(0, 3fr) 24px; gap: 8px; align-items: center }
.mac .hv-env .dbtn { padding: 0; min-width: 24px; color: var(--d-muted) }
.mac .hv-stat { display: flex; gap: 8px; align-items: center; font-size: 12.5px; color: var(--d-muted); padding: 6px 0 }
.mac .hv-acts { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin-top: 6px }
.mac .hv-note { font-size: 12px; line-height: 17px; color: var(--d-muted); margin-top: 8px }
.mac .hv-note.bad { color: var(--d-err) }
.mac .hv-diff { display: inline-block; margin-left: 6px; padding: 0 6px; border-radius: 999px; border: 1px solid var(--d-border); font-size: 10.5px; line-height: 15px; font-weight: 400; color: var(--d-muted); cursor: help; vertical-align: 1px }
.mac .hv-save { margin-top: 12px }
.mac .hv-incl { display: inline-flex; gap: 6px; align-items: center; font-size: 12px; color: var(--d-muted); white-space: nowrap }
.mac .hv-ok { font-size: 12px; color: var(--d-emerald) }
.mac .hv-acts [hidden] { display: none }
.mac .hv-pick { display: inline-flex; min-width: 0 }
.mac .hv-seg { display: inline-flex; flex-wrap: nowrap; gap: 2px; padding: 2px; min-width: 0; border: 1px solid var(--d-border); border-radius: var(--radius-sm); background: color-mix(in srgb, var(--d-fg) 2%, transparent) }
.mac .hv-seg button { all: unset; cursor: pointer; height: 24px; line-height: 24px; padding: 0 10px; border-radius: var(--radius-sm); font-size: 12.5px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 150px; min-width: 0 }
.mac .hv-seg button:hover:not(:disabled) { color: var(--d-fg) }
.mac .hv-seg button[aria-pressed="true"] { background: color-mix(in srgb, var(--d-emerald) 28%, transparent); color: var(--d-fg) }
.mac .hv-seg button:disabled { opacity: .45; cursor: default }
.mac .hv-seg button:focus-visible { box-shadow: inset 0 0 0 2px var(--d-emerald) }
.mac .hv-sel { display: none; height: 28px; max-width: 220px; min-width: 0; padding: 0 8px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: var(--d-surface); color: var(--d-fg); font: inherit; font-size: 12.5px }
.mac .hv-pick.crowded .hv-seg { display: none }
.mac .hv-pick.crowded .hv-sel { display: block }
.mac .hv-bar {  display: flex; align-items: center; gap: 8px; padding-bottom: 12px; border-bottom: 1px solid var(--d-border); min-width: 0 }
.mac .hv-bar .lbl { flex: none; font-size: 12px; color: var(--d-muted) }
.mac .hv-bar .sp { flex: 1 }
.mac .hv-bar .dbtn { flex: none; white-space: nowrap }
.mac .hv-sum { display: grid; grid-template-columns: 28px minmax(0, 1fr) auto 36px; gap: 12px; align-items: center; padding: 12px 14px; border: 1px solid var(--d-border); border-radius: var(--radius) }
.mac .hv-sum b { font-size: 15px; font-weight: 600 }
.mac .hv-sum .on { font-size: 12px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 200px }
.mac .hv-view.in-set {  }
.mac .hv-view.in-set > .hv-d { padding: 0; overflow: visible }
.mac .hv-view.in-set > .hv-d > .hv-acts:first-child, .mac .hv-view.in-set > .hv-d > .hv-note:first-child { margin-top: 0 }
.mac .fl-seg, .mac .fl-chips { display: flex; flex-wrap: wrap; gap: 4px }
.mac .fl-chips .dchip { font-family: var(--mono); font-size: 11.5px }
.mac .fl-row { display: flex; gap: 8px; align-items: center }
.mac .fl-tbl { width: 100%; table-layout: fixed; border-collapse: collapse; margin-top: 10px; border: 1px solid var(--d-border); border-radius: var(--radius) }
.mac .fl-tbl th { text-align: left; font-size: 12px; font-weight: 500; color: var(--d-muted); padding: 8px 10px; border-bottom: 1px solid var(--d-border) }
.mac .fl-tbl td { padding: 6px 10px; border-bottom: 1px solid var(--d-border); vertical-align: middle }
.mac .fl-tbl th:nth-child(2) { width: 52px }
.mac .fl-tbl th:nth-child(3) { width: 32% }
.mac .fl-tbl th:nth-child(4) { width: 96px }
.mac .fl-tbl th { white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .fl-tbl td:last-child { text-align: right; white-space: nowrap }
.mac .model-context-scroll { overflow-x: auto }
.mac .model-context { min-width: 560px }
.mac .model-context th:nth-child(2), .mac .model-context th:nth-child(3) { width: 120px }
.mac .model-context th:nth-child(4) { width: 144px }
.mac .model-context td { font-variant-numeric: tabular-nums; overflow-wrap: anywhere }
.mac .model-context td small { display: block; color: var(--d-muted); font-size: 12px }
.mac .model-context td:last-child { text-align: left }

.mac button.cc-note.in-go { width: 100%; text-align: left; font: inherit; font-size: 12.5px; cursor: pointer }
.mac button.cc-note.in-go:hover { border-color: var(--d-border-hi) }
.mac .cc-note.in-go .go { margin-left: auto; color: var(--d-emerald); white-space: nowrap }
.mac .dtag { font-size: 10.5px; font-weight: 600; padding: 1px 6px; border-radius: var(--radius-sm); background: var(--d-elev); color: var(--d-muted) }
.mac .dswitch { width: 30px; height: 18px; border-radius: var(--radius-sm); background: var(--d-border); position: relative; border: 0; padding: 0; flex: none }
.mac .dswitch::after { content: ""; position: absolute; left: 2px; top: 2px; width: 14px; height: 14px; border-radius: 50%; background: var(--d-fg); transition: transform .2s var(--ease) }
.mac .dswitch[aria-checked="true"] { background: var(--d-emerald) }
.mac .dswitch[aria-checked="true"]::after { transform: translateX(12px) }
.mac .dtoggle { display: flex; gap: 12px; align-items: center }
.mac .dtoggle span { flex: 1; display: grid; line-height: 19px }
.mac .dtoggle b { font-weight: 500; font-size: 13.5px }
.mac .dtoggle small { font-size: 12px; color: var(--d-muted) }
.mac .dtoggle + .dtoggle { margin-top: 10px }
.mac .dsec.flat { padding: 0; border: 0 }
.mac .dchip { min-height: 26px; padding: 0 10px; border-radius: 999px; border: 1px solid var(--d-border); background: var(--d-surface); color: var(--d-fg); font-size: 12.5px }
.mac .dchip[aria-pressed="true"] { background: color-mix(in srgb, var(--d-emerald) 28%, transparent); border-color: var(--d-emerald) }
.mac .dchip.day { min-width: 34px; padding: 0 8px }
.mac .dchip:disabled { opacity: .5 }
.mac .dapps { padding-top: 4px }
.mac .dapps .dsec { border: 0; padding-left: 10px }
.mac .dalogo { width: 28px; height: 28px; border-radius: var(--radius-sm); flex: none; object-fit: contain; background: var(--d-elev); display: grid; place-items: center; font-weight: 600; font-size: 13px; color: var(--d-muted) }
.mac .dappr, .mac .dlevel, .mac .daction { display: flex; gap: 10px; align-items: center; padding: 6px 10px; border-radius: var(--radius-sm); min-height: 42px }
.mac .dappr .tt, .mac .dlevel .tt { flex: 1; min-width: 0; display: grid; line-height: 18px }
.mac .dappr .tt b, .mac .dlevel .tt b { font-size: 13.5px; font-weight: 500 }
.mac .dappr .tt small, .mac .dlevel .tt small { font-size: 12px; color: var(--d-muted); white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .dappr.on { cursor: pointer }
.mac .dappr.on:hover, .mac .dlevel:hover { background: var(--d-elev) }
.mac .dappr .dmuted { display: inline-flex; gap: 6px; align-items: center; font-size: 12px }
.mac .dlevel { width: 100%; border: 0; background: none; color: inherit; text-align: left; font: inherit; cursor: pointer }
.mac .daction span:first-child { flex: 1; font-size: 13.5px }
.mac .daction span:first-child small { display: block; font-size: 12px; color: var(--d-muted) }
.mac .daction.off span:first-child { color: var(--d-muted) }
.mac .dsplit { display: flex; justify-content: space-between }
.mac .dsplit .over { color: var(--d-err) }
.mac .dtag.ok { color: var(--d-emerald) }
.mac .dtag.ask { color: #f2994a; align-self: auto; max-width: none; padding: 1px 6px; border-radius: var(--radius-sm); background: var(--d-elev); white-space: nowrap }
.mac .dappr .dtag, .mac .dappr .dbtn { flex: none; white-space: nowrap }
.mac .dskel-rows { display: grid; gap: 12px; padding: 10px 0 12px }
.mac .dskel { height: 10px; border-radius: 5px; background: var(--d-elev) }
.mac .dgit { display: grid; grid-template-columns: 18px minmax(0, 1fr); column-gap: 8px; row-gap: 6px; align-items: center; padding: 8px 0; border-bottom: 1px solid var(--d-border); font-size: 13px }
.mac .dgit > :not(.ic) { grid-column: 2 }
.mac .dgit .ic { display: grid; place-items: center; color: var(--d-fg) }
.mac .dgit.off .ic { opacity: .45 }
.mac .dgit .l1 { display: flex; align-items: center; gap: 8px; min-width: 0 }
.mac .dgit .l1 b { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .dgit .acct { color: var(--d-muted); font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis }
.mac .dgit .acts { margin-left: auto; display: flex; gap: 4px; align-items: center; flex: none }
.mac .dgit .l2 { display: flex; flex-wrap: wrap; gap: 4px }
.mac .dgchip { font-size: 11px; line-height: 18px; padding: 0 7px; border-radius: 999px; border: 1px solid var(--d-border); color: var(--d-muted); white-space: nowrap; flex: none }
.mac .dgchip.warn { color: #f2c94c; border-color: color-mix(in srgb, #f2c94c 40%, transparent) }
.mac .dgchip.err { color: var(--d-err); border-color: color-mix(in srgb, var(--d-err) 45%, transparent) }
.mac .dbtn.dico { width: 26px; min-height: 24px; padding: 0; justify-content: center }
.mac .dskins { display: grid; gap: 6px; margin-bottom: 12px }
.mac .dskins small { font-size: 12px; color: var(--d-muted) }
.mac .dskins > div { display: flex; flex-wrap: wrap; gap: 6px }
.mac .dskin { display: inline-flex; align-items: center; gap: 8px; padding: 5px 11px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: transparent; color: var(--d-muted); font-size: 13px; cursor: pointer }
.mac .dskin i { width: 12px; height: 12px; border-radius: 3px; border: 1px solid color-mix(in srgb, var(--d-muted) 50%, transparent) }
.mac .dskin:hover { color: var(--d-fg) }
.mac .dskin[aria-pressed="true"] { background: var(--d-elev); color: var(--d-fg); border-color: var(--d-emerald) }
.mac .dskin:focus-visible { outline: 2px solid var(--d-emerald); outline-offset: 2px }
.mac .cc-set-scrim { position: absolute }
.mac .cc-set .dset-nav .cc-si .n { margin-left: auto; font-size: 11px; color: var(--d-dim) }
.mac .dlradio { width: 14px; height: 14px; border-radius: 50%; border: 2px solid var(--d-border); flex: none }
.mac .dlevel[aria-checked="true"] .dlradio { border-color: var(--d-emerald); box-shadow: inset 0 0 0 3px var(--d-surface), inset 0 0 0 7px var(--d-emerald) }
.mac .dset-page select, .mac .dform .dtoggle select { min-height: 28px; padding: 0 8px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: var(--d-surface); color: var(--d-fg); font: inherit; font-size: 13px }
.mac .dset-page input:not([type="checkbox"]), .mac .dsetcol input:not([type="checkbox"]) { min-height: 30px; padding: 0 10px; border-radius: var(--radius-sm); border: 1px solid var(--d-border); background: var(--d-surface); color: var(--d-fg); font: inherit; font-size: 13px; outline: 0; min-width: 0 }
.mac .dset-page input.hv-in, .mac .hv-view input.hv-in { height: 32px; min-height: 0; padding: 0 11px; background: var(--d-ink); font-family: var(--mono); font-size: 12.5px; width: 100%; box-sizing: border-box }
.mac .dsetcol { display: flex; flex-direction: column; overflow-y: auto; min-height: 0; scrollbar-width: thin }
.mac .in-ed textarea { width: 100%; box-sizing: border-box }
.mac .in-hist { display: block }
.mac .in-hist + .in-hist { border-top: 0 }
.mac .hv-view { padding: 16px 18px; overflow: auto; min-height: 0 }
.mac .cc-set .hv-view, .mac .cc-set .in-view { padding: 0; overflow: visible }`;
  document.head.append(st);

  const chev = DI.chevR;
  const tile = a => dMacAgTile(a, "cc-ag");
  const sm = 'style="font-size:11.5px;padding:2px 8px;min-height:24px"';
  const dt = (name, sub, end = "") => `<div class="dtoggle"><span><b>${name}</b><small>${sub}</small></span>${end}</div>`;
  const swi = (on, label, attrs = "") => `<button class="dswitch" role="switch" aria-checked="${on}" aria-label="${label}" ${attrs}></button>`;
  const sec = (title, body) => `<section><div class="dsec flat" style="padding-top:6px">${title}</div><div>${body}</div></section>`;
  const skel = n => `<div class="dskel-rows">${[78, 56, 68, 48].slice(0, n).map(w => `<div class="dskel" style="width:${w}%"></div>`).join("")}</div>`;
  const dsetGo = k => `data-dact="dset:set:${k}" data-dgo="dWinSettings"`;

  // General
  const generalBody = () => {
    const cur = S.d.vn || "ask", word = S.d.vsend === "word", agn = !S.d.agnoff;
    return dt("Launch at login", "Keep Beep in the menu bar after a restart", swi(S.d.login !== false, "Launch at login", 'data-dact="login"'))
      + dt("Voice notes", VN_SAYS[cur])
      + `<div class="dchips" role="radiogroup" aria-label="Voice notes" style="margin:6px 0 10px">${VN_CHOICES.map(([k, label]) => `<button class="dchip" role="radio" aria-checked="${cur === k}" aria-pressed="${cur === k}" data-dact="dset:vn:${k}">${label}</button>`).join("")}</div>`
      + dt("When I stop talking", word ? "Pauses never send. Say “send” or “stop” when you are done." : "Sends after a short pause. Saying “send” sends at once.")
      + `<div class="dchips" role="radiogroup" aria-label="When I stop talking" style="margin:6px 0 10px"><button class="dchip" role="radio" aria-checked="${!word}" aria-pressed="${!word}" data-dact="dset:vsend:pause">Send when I stop talking</button><button class="dchip" role="radio" aria-checked="${word}" aria-pressed="${word}" data-dact="dset:vsend:word">Wait until I say “send”</button></div>`
      + dt("Coding agent notifications", agn ? "When a coding agent asks something or finishes a turn" : "Coding agents only show under Waiting", swi(agn, "Coding agent notifications", `data-dact="dset:agnoff:${S.d.agnoff ? "" : "1"}"`))
      + dt("Keyboard shortcuts", "Every shortcut, by where it works", '<button class="dbtn" data-dgo="dShortcuts">Show…</button>')
      + dt("Report a problem", "Collect logs from your machines and send them to us", '<button class="dbtn" data-dgo="dReport">Report…</button>');
  };

  // Appearance
  const SK = [["flashmind", "Flashmind", "#0d161e"], ["linear", "Linear", "#151617"], ["classic", "Classic", "#1a2a23"], ["quiet", "Quiet", "#161b18"], ["github", "GitHub dark", "#161b22"], ["zed", "Zed", "#2f343e"], ["tokyo", "Tokyo Night", "#1f2335"], ["mocha", "Catppuccin Mocha", "#313244"], ["nord", "Nord", "#3b4252"], ["gruvbox", "Gruvbox", "#3c3836"], ["rosepine", "Rose Pine", "#26233a"], ["geist", "Geist", "#000"], ["mono", "High contrast", "#000"]];
  const SKL = [["paper", "Light", "#f3f3f1"], ["solarized", "Solarized", "#eee8d5"], ["macos", "macOS", "#e9e7ea"], ["warm", "Warm", "#f3efe8"]];
  const skinsHtml = () => {
    const on = S.d.skin || "linear";
    const group = (title, list) => `<div class="dskins"><small>${title}</small><div>${list.map(([id, n, c]) => `<button class="dskin" aria-pressed="${id === on}" ${id === "linear" || id === "paper" || id === "flashmind" ? `data-dact="dset:skin:${id === "linear" ? "" : id}" data-dgo="dAppearance"` : `data-toast="Skin: ${n}"`}><i style="background:${c}"></i>${n}</button>`).join("")}</div></div>`;
    return group("Dark", SK) + group("Light", SKL);
  };

  // Models
  const CHOICES = [["Claude Haiku 5.5", "$1 in, $5 out per M tokens"], ["GPT-6 Luna", "$1.25 in, $10 out per M tokens"], ["GLM 5.3 Flash", "$0.2 in, $0.8 out per M tokens, text only"]];
  const modelsBody = () => {
    const auto = S.d.setAuto !== "off";
    const pick = CHOICES[1];
    return dt("Model", pick[1], `<select aria-label="Model">${CHOICES.map(([n], i) => `<option${i === 1 ? " selected" : ""}>${n}</option>`).join("")}</select>`)
      + dt("Auto thinking", auto ? "Beep picks the thinking for each prompt" : "Every prompt thinks as set below", swi(auto, "Auto thinking", `data-dact="dset:setAuto:${auto ? "off" : "on"}"`))
      + `<div class="dfield"><span>Thinking${auto ? " (Auto picks it per prompt)" : ""}</span><div class="dchips">${["Off", "Low", "Medium", "High"].map(l => `<button class="dchip" aria-pressed="${!auto && l === "Medium"}"${auto ? " disabled" : ""} data-toast="Thinking: ${l}">${l}</button>`).join("")}</div></div>`;
  };

  // Apps
  const letter = n => `<span class="dalogo">${n.slice(0, 1).toUpperCase()}</span>`;
  const waiting = act => `<span class="dmuted"><span class="dspin"></span> Waiting for browser…</span><button class="dbtn quiet" data-dact="${act}">Cancel</button>`;
  const appsList = sheet => {
    const s = S.d.apps || "list", wait = S.d.appWait || "Notion";
    const open = n => sheet ? `data-dact="dset:set:app-gmail" data-dgo="dWinSettings"` : `data-dgo="dAppPerm" aria-label="${n} permissions"`;
    const row = ([n, l, c, desc], end, attrs = "") => `<div class="dappr${attrs ? " on" : ""}" ${attrs}${attrs ? ' role="button" tabindex="0"' : ""}>${dLogo(l, c)}<span class="tt"><b>${n}</b><small>${desc}</small></span>${end}</div>`;
    const none = s === "none";
    const connected = none ? "" : D_APPS.slice(0, 2).map(a => row(a, `<span class="dtag">Connected</span><span class="dmuted">${chev}</span>`, open(a[0]))).join("");
    const more = (none ? D_APPS : D_APPS.slice(2)).map(a => row(a, s === "nokey" ? `<span class="dmuted">Unavailable</span>`
      : s === "waiting" && a[0] === wait ? waiting("appCancel")
      : `<button class="dbtn pri" data-dact="appConnect:${a[0]}" data-toast="Opening ${a[0]} sign-in in the browser"${s === "waiting" ? " disabled" : ""}>Connect</button>`)).join("");
    const server = ([n, l, host, tools]) => {
      const end = s === "mcpWait" && !tools ? waiting("appCancel")
        : tools ? `<span class="dtag ok">${tools} tools</span><span class="dmuted">${chev}</span>`
        : `<span class="dtag ask">Sign-in expired</span><button class="dbtn pri" data-toast="Opening ${n} sign-in in the browser">Sign in</button><button class="dbtn quiet warn" data-toast="${n} removed">Remove</button>`;
      const go = tools && s !== "mcpWait" ? (sheet ? `data-dact="dset:set:mcp-wiki" data-dgo="dWinSettings"` : `data-dgo="dMcp" aria-label="${n} tools"`) : "";
      return `<div class="dappr${go ? " on" : ""}" ${go}${go ? ' role="button" tabindex="0"' : ""}>${letter(n)}<span class="tt"><b>${n}</b><small>${host}</small></span>${end}</div>`;
    };
    const down = s === "mcpDown" ? `<div class="dappr">${letter("Docs")}<span class="tt"><b>Docs</b><small>docs.example.org</small></span><span class="dtag ask">Not connected</span><button class="dbtn pri" data-toast="Connecting to Docs">Retry</button><button class="dbtn quiet warn" data-toast="Docs removed">Remove</button></div>` : "";
    const add = s === "mcpAdd" || s === "mcpBad"
      ? `<div class="dform dmcp"><label class="dfield"><span>Name</span><input maxlength="60" placeholder="Team notes" autocomplete="off" spellcheck="false" value="Team notes"></label>
        <label class="dfield${s === "mcpBad" ? " err" : ""}"><span>${s === "mcpBad" ? "Use an https address on the public internet" : "Server URL"}</span><input class="url" maxlength="2000" placeholder="https://example.com/mcp" autocomplete="off" spellcheck="false"${s === "mcpBad" ? ' value="http://example.com/mcp"' : ""}></label>
        <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri" data-toast="Connecting to Team notes">Add server<kbd>${DI.ret}</kbd></button><button class="dbtn quiet" data-dact="mcpCancel">Cancel</button></div></div>`
      : `<div style="padding:4px 10px"><button class="dbtn" data-dact="mcpAdd">Add MCP server</button></div>`;
    return `<div class="dlist dapps">${connected ? `<div class="dsec">Connected</div>${connected}` : ""}<div class="dsec">${connected ? "More apps" : "Available"}</div>${more}
      <div class="dsec">Your MCP servers</div>${MCP_SERVERS.map(server).join("")}${down}${add}
      <div class="dmuted" style="padding:6px 10px;display:block">Beep asks before every tool on a server you add.</div></div>`;
  };
  const appPerm = (fm, back) => {
    const lvl = S.d.appLevel || "ask";
    const allowed = S.d.appAllowed || (S.d.appAllowed = { "Send email": false, "Reply to email": true, "Move to trash": false, "Save a draft": true });
    const levels = [["ask", "Ask first", "Beep shows what it is about to do and waits for you."], ["allow", "Always allow", "Beep sends, changes and deletes without asking."], ["read", "Read only", "Beep can search and read, never change anything."]];
    const [an, al, ac] = fm ? ["Fastmail", FM_LOGO, "#fff"] : ["Gmail", "G", "#F28B7D"];
    const accts = fm === "one" || fm === "removeone" ? ["alex@example.com"] : ["alex@example.com", "alex@work.example"];
    const accounts = !fm ? "" : fm === "failed"
      ? `<div class="dsec">Accounts</div><div class="daction"><span class="derr">Fastmail accounts did not load: the server took too long</span></div><div style="padding:4px 10px"><button class="dbtn" data-dact="fm:two">Retry</button></div>`
      : `<div class="dsec">Accounts</div>${accts.map((a, i) => {
          const asking = (fm === "remove" && i === 1) || fm === "removeone";
          return `<div class="daction"><span>${a}${i === 0 && accts.length > 1 ? ' <small class="dmuted" style="display:inline">Default</small>' : ""}</span>${asking
            ? `<span class="dmuted">${accts.length === 1 ? "Disconnects Fastmail" : "Beep stops using this account"}</span><button class="dbtn quiet" data-dact="fm:${accts.length === 1 ? "one" : "two"}">Cancel</button><button class="dbtn warn" ${accts.length === 1 ? `data-dgo="dApps" data-toast="Fastmail disconnected"` : `data-dact="fm:one" data-toast="alex@work.example removed"`}>Remove</button>`
            : `<button class="dbtn quiet warn" data-dact="fm:${accts.length === 1 ? "removeone" : "remove"}">Remove</button>`}</div>`;
        }).join("")}${fm === "adding" ? `<div style="padding:4px 10px;display:flex;gap:8px;align-items:center">${waiting("fm:two")}</div>` : `<div style="padding:4px 10px"><button class="dbtn" data-dact="fm:adding" data-toast="Opening Fastmail sign-in in the browser">Add another account</button></div>`}
        <div class="dmuted" style="padding:6px 10px;display:block">Beep uses the first account unless you name another, like "check my work inbox". To add a different account, sign out of Fastmail in the browser first.</div>`;
    const tag = a => lvl === "read" ? `<span class="dtag">Off</span>` : lvl === "allow" || allowed[a] ? `<span class="dtag ok">Allowed</span>` : `<span class="dtag ask">Asks</span>`;
    const acts = Object.keys(allowed).map(a => `<div class="daction"><span>${a}</span>${tag(a)}${lvl === "ask" ? swi(allowed[a], `Always allow ${a}`, `data-dact="appAllow:${a}"`) : ""}</div>`).join("");
    return `<div class="dhead">${dLogo(al, ac)}<span class="tt"><b>${an}</b><small>What Beep may do here</small></span></div>
      <div class="dlist dapps">${accounts}<div class="dsec">When Beep acts in ${an}</div>${levels.map(([k, t, d]) => `<button class="dlevel" role="radio" aria-checked="${lvl === k}" data-dact="appLevel:${k}"><span class="dlradio"></span><span class="tt"><b>${t}</b><small>${d}</small></span></button>`).join("")}
      <div class="dsec">Actions</div>${acts}
      <div class="dmuted" style="padding:8px 10px;display:block">Searching and reading is always allowed.</div>
      <div style="padding:6px 10px 2px"><button class="dbtn warn" ${back} data-toast="${an} disconnected">Disconnect ${an}${fm && accts.length > 1 ? " (all accounts)" : ""}</button></div></div>`;
  };
  const mcpPage = back => {
    const tools = S.d.mcpMany ? MCP_MANY : MCP_FEW;
    if (!S.d.mcpOn) { S.d.mcpOn = {}; tools.filter(t => !MCP_TOO_LARGE.includes(t)).slice(0, MCP_LIMIT).forEach(t => S.d.mcpOn[t] = true); }
    const on = S.d.mcpOn, count = tools.filter(t => on[t]).length;
    const rows = tools.map(t => {
      if (S.d.mcpAsk === t) return `<div class="daction"><span>${t}<small>More than ${MCP_LIMIT} tools makes replies slower and tool picks worse</small></span><button class="dbtn quiet" data-dact="mcpNo">Cancel</button><button class="dbtn pri" data-dact="mcpYes">Turn on</button></div>`;
      if (MCP_TOO_LARGE.includes(t)) return `<div class="daction off"><span>${t}<small>Too large for Beep to use</small></span>${swi(false, `Use ${t}`, "disabled")}</div>`;
      return `<div class="daction"><span>${t}</span>${swi(!!on[t], `Use ${t}`, `data-dact="mcpTool:${t}"`)}</div>`;
    }).join("");
    return `<div class="dhead">${letter("Team wiki")}<span class="tt"><b>Team wiki</b><small class="dmono">https://wiki.example.com/mcp</small></span></div>
      <div class="dlist dapps"><div class="dsec dsplit"><span>Tools</span><span${count > MCP_LIMIT ? ' class="over"' : ""}>${count} of ${MCP_LIMIT} on</span></div>${rows}
      <div class="dmuted" style="padding:8px 10px;display:block">Beep uses up to ${MCP_LIMIT} tools from a server, so turn on the ones you need. It asks before every tool on a server you add.</div>
      <div style="padding:6px 10px 2px;display:flex;gap:8px;align-items:center">${S.d.mcpRm ? `<span class="dmuted">Beep stops using this server</span><button class="dbtn quiet" data-dact="dset:mcpRm:">Cancel</button><button class="dbtn warn" ${back} data-toast="Team wiki removed">Remove</button>` : `<button class="dbtn warn" data-dact="dset:mcpRm:1">Remove server</button>`}</div></div>`;
  };

  // This Mac: thisMac(mac) in settings.ts.
  const macBody = () => dThisMac();

  // Agent accounts: agentaccounts.ts.
  const box = inner => `<div style="border:1px solid var(--d-border);border-radius:10px;padding:12px 14px;margin-top:12px">${inner}</div>`;
  const mrow = (name, off, act) => `<div style="display:flex;align-items:center;gap:8px;padding:6px 0;font-size:12.5px"><span class="ddot${off ? " off" : ""}"></span><span>${name}</span><span style="margin-left:auto;color:var(--d-muted);display:flex;gap:6px;align-items:center">${act}</span></div>`;
  const agentForm = agent => {
    const claude = agent === "claude";
    const note = claude ? `Run <code style="font-size:11px">claude setup-token</code> in Terminal and paste the token it prints.` : "An OpenAI API key. Codex uses it on every machine instead of signing in.";
    return `<div style="display:grid;gap:8px;margin-top:10px"><div style="display:flex;gap:8px"><input type="password" autocomplete="off" spellcheck="false" placeholder="${claude ? "sk-ant-oat01-…" : "sk-…"}" aria-label="${claude ? "Claude Code token" : "Codex API key"}" style="flex:1;font-family:var(--mono,monospace);font-size:12px">
      <button class="dbtn pri" data-toast="Saved">Save</button><button class="dbtn quiet" ${dsetGo("acct")}>Cancel</button></div><span style="font-size:11.5px;color:var(--d-muted)">${note}</span></div>`;
  };
  const claudeBox = v => {
    const head = sub => `<div style="display:flex;align-items:center;gap:10px">${tile("claude")}<span style="display:grid;gap:2px;min-width:0"><b style="font-size:13.5px">Claude Code</b>${sub ? `<span style="font-size:11.5px;color:var(--d-muted)">${sub}</span>` : ""}</span>`;
    if (v === "acct-flow") return box(`${head("")}<span style="margin-left:auto"><button class="dbtn quiet" ${sm} ${dsetGo("acct-none")}>Cancel</button></span></div>
      <div style="font-size:12px;color:var(--d-muted);margin-top:8px;display:flex;gap:8px;align-items:center"><span class="dspin"></span>Approve in your browser. Beep saves the token once you do.</div>`);
    if (v === "acct-none" || v === "acct-unread" || v === "acct-paste-none") {
      const why = v === "acct-unread" ? "Beep can no longer read the saved token. Sign in again." : "Sign in for all to give every machine one token. Beep keeps it encrypted and never writes it to their disks.";
      const edit = v === "acct-paste-none";
      return box(`${head("signs in on each machine")}<span style="margin-left:auto;display:flex;gap:6px;flex:none"><button class="dbtn" ${dsetGo("acct-flow")}>Sign in for all</button>${edit ? "" : `<button class="dbtn quiet" ${dsetGo("acct-paste-none")}>Paste a token</button>`}</span></div>
        <div style="margin-top:6px">${mrow("This Mac", false, "signed in as you@example.com")}${mrow("Workspace", false, "not signed in")}${mrow("gpu-box", true, "not checked")}</div><div style="font-size:12px;color:var(--d-muted);margin-top:6px">${why}</div>${edit ? agentForm("claude") : ""}`);
    }
    const edit = v === "acct-paste";
    return box(`${head(`signed in today · <code style="font-size:11px">…a91f</code>`)}<span style="margin-left:auto;display:flex;gap:6px;flex:none"><button class="dbtn" ${dsetGo("acct-flow")}>Replace</button><button class="dbtn warn" ${dsetGo("acct-none")} data-toast="Revoked">Revoke</button></span></div>
      ${edit ? agentForm("claude") : ""}<div style="font-size:12px;color:var(--d-muted);margin:8px 0 4px">Used on</div>
      ${mrow("build-box", false, "2 sessions now")}${mrow("Workspace", false, "idle")}${mrow("This Mac", false, "1 session now")}${mrow("gpu-box", true, "offline · gets it when back")}
      ${edit ? "" : `<button class="dbtn quiet" ${sm} ${dsetGo("acct-paste")}>Paste a token instead</button>`}`);
  };
  const codexBox = v => {
    const key = v === "acct-apikey", form = v === "acct-keyform";
    const sub = key ? `API key <code style="font-size:11px">…7c2e</code> on every machine · added 3 days ago` : "signs in on each machine";
    const acts = key ? `<button class="dbtn" ${sm} ${dsetGo("acct-keyform")}>Replace key</button><button class="dbtn quiet warn" ${sm} ${dsetGo("acct")}>Remove key</button>` : `<button class="dbtn" ${dsetGo("acct-keyform")}>Use an API key</button>`;
    const flow = `<div style="padding:8px 10px;margin:4px 0;border-radius:8px;background:var(--d-elev);border:1px solid var(--d-border);font-size:12.5px;display:grid;gap:6px"><span>Workspace: open <b>auth.openai.com/codex/device</b> and enter</span><span style="font-family:var(--mono,monospace);font-size:18px;letter-spacing:.12em;user-select:all">K7QD-M2XR</span><span style="display:flex;gap:6px;align-items:center;color:var(--d-muted)"><span class="dspin"></span>Waiting for you to approve · 14:32 left<span style="margin-left:auto;display:flex;gap:6px"><button class="dbtn" ${sm} data-toast="Copied">Copy code</button><button class="dbtn quiet" ${sm} data-toast="Opens the page">Open page</button><button class="dbtn quiet" ${sm} ${dsetGo("acct")}>Cancel</button></span></span></div>`;
    const work = v === "acct-code" ? flow
      : v === "acct-start" ? mrow("Workspace", false, '<span class="dspin"></span>starting sign-in')
      : mrow("Workspace", false, `<span>not checked</span><button class="dbtn" ${sm} ${dsetGo("acct-code")}>Sign in</button>`) + (v === "acct-signfail" ? '<div class="derr" style="padding-left:15px">Workspace could not start codex login</div>' : "");
    return box(`<div style="display:flex;align-items:center;gap:10px">${tile("codex")}<span style="display:grid;gap:2px;min-width:0"><b style="font-size:13.5px">Codex</b><span style="font-size:11.5px;color:var(--d-muted)">${sub}</span></span><span style="margin-left:auto;display:flex;gap:6px;flex:none">${form ? "" : acts}</span></div>
      ${form ? agentForm("codex") : ""}<div style="margin-top:6px">${mrow("build-box", false, "signed in")}${mrow("This Mac", false, "signed in")}${work}${mrow("gpu-box", true, "offline")}</div>`);
  };
  const acctBody = v => v === "acct-fail" ? `<div class="dmuted">Could not load your agent accounts.</div>`
    : v === "acct-old" ? `<div class="dmuted">The server needs an update before it keeps agent accounts.</div>`
    : v === "acct-load" ? skel(2)
    : `<div style="font-size:12.5px;color:var(--d-muted);margin-top:4px">How your coding agents sign in on your machines.</div>${claudeBox(v)}${codexBox(v)}`;
  const CODING = [["claude", "Claude Code"], ["codex", "Codex"], ["gemini", "Gemini CLI"], ["pi", "pi"], ["opencode", "OpenCode"], ["flsh", "flsh"]];
  const codingAgents = () => `<div class="dset-h">Coding agents</div><div class="dset-box">${CODING.map(([a, n]) => `<div class="dset-row" role="button" tabindex="0" ${dsetGo(`acct-${a}`)}>${tile(a)}<span class="tt"><b>${n}</b><small>Version and updates, on or off, binary path, launch arguments and env vars on each machine</small></span>${chev}</div>`).join("")}</div>`;

  // Credentials: credentials.ts.
  const GH = `<svg viewBox="0 0 16 16" width="15" height="15" fill="currentColor" aria-hidden="true"><path d="M8 0c4.42 0 8 3.58 8 8a8.013 8.013 0 0 1-5.45 7.59c-.4.08-.55-.17-.55-.38 0-.27.01-1.13.01-2.2 0-.75-.25-1.23-.54-1.48 1.78-.2 3.65-.88 3.65-3.95 0-.88-.31-1.59-.82-2.15.08-.2.36-1.02-.08-2.12 0 0-.67-.22-2.2.82-.64-.18-1.32-.27-2-.27-.68 0-1.36.09-2 .27-1.53-1.03-2.2-.82-2.2-.82-.44 1.1-.16 1.92-.08 2.12-.51.56-.82 1.28-.82 2.15 0 3.06 1.86 3.75 3.64 3.95-.23.2-.44.55-.51 1.07-.46.21-1.61.55-2.33-.66-.15-.24-.6-.83-1.23-.82-.67.01-.27.38.01.53.34.19.73.9.82 1.13.16.45.68 1.31 2.69.94 0 .67.01 1.3.01 1.49 0 .21-.15.45-.55.38A7.995 7.995 0 0 1 0 8c0-4.42 3.58-8 8-8Z"/></svg>`;
  const FJ = `<svg viewBox="0 0 212 212" width="15" height="15" aria-hidden="true"><g fill="none" transform="translate(6 6)"><path d="M58 168V70a50 50 0 0 1 50-50h20" stroke="#f60" stroke-width="25"/><path d="M58 168v-30a50 50 0 0 1 50-50h20" stroke="#d40000" stroke-width="25"/><circle cx="142" cy="20" r="18" stroke="#f60" stroke-width="15"/><circle cx="142" cy="88" r="18" stroke="#d40000" stroke-width="15"/><circle cx="58" cy="180" r="18" stroke="#d40000" stroke-width="15"/></g></svg>`;
  const ED = `<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M17 3a2.85 2.85 0 0 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/></svg>`;
  const RM = `<svg viewBox="0 0 24 24" width="13" height="13" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4h8v2M19 6l-1 14H6L5 6"/></svg>`;
  const chip = (t, title = "", kind = "") => `<span class="dgchip${kind ? ` ${kind}` : ""}"${title ? ` title="${title}"` : ""}>${t}</span>`;
  const ico = (i, label, attr) => `<button class="dbtn quiet dico" title="${label}" aria-label="${label}" ${attr}>${i}</button>`;
  const editRm = (what, editKey) => ico(ED, `Replace the token for ${what}`, dsetGo(editKey)) + ico(RM, `Remove ${what}`, `data-toast="${what} removed"`);
  const cliChip = (cli, ms) => ms.length ? chip(ms.length === 1 ? `${cli} on ${ms[0]}` : `${cli} on ${ms.length} machines`, ms.join(", ")) : chip(`${cli} on no machine yet`);
  const gitRow = ({ icon, name, account, tag, usable = true, acts, chips, more }) => `<div class="dgit${usable ? "" : " off"}"><span class="ic">${icon}</span>
    <div class="l1"><b>${name}</b>${account ? `<span class="acct">${account}</span>` : ""}${tag || ""}<span class="acts">${acts}</span></div>${chips ? `<div class="l2">${chips}</div>` : ""}${more || ""}</div>`;
  const note = html => `<span style="font-size:11.5px;color:var(--d-muted)">${html}</span>`;
  const lk = (href, text) => `<a href="#" style="color:var(--d-emerald)" data-toast="Opens ${href} in the browser">${text}</a>`;
  const cd = t => `<code style="font-size:11px">${t}</code>`;
  const MS = ["build-box", "mac-box", "gpu-box"];
  const credForm = ({ title, url, urlField, check, label = "Check and save", html, err }) => `<div class="dcred-form" style="padding:10px 0;border-bottom:1px solid var(--d-border);display:grid;gap:8px">
    <div style="display:flex;align-items:center;gap:8px;font-size:13px"><b style="font-weight:600">${title}</b>${url ? `<span style="color:var(--d-muted);font-size:12px">${url}</span>` : ""}</div>
    <div class="dfield${err ? " err" : ""}" style="gap:8px">${urlField ? `<input class="url" type="url" placeholder="https://git.example.com" autocomplete="off" spellcheck="false" aria-label="Server URL">` : ""}
      <div style="display:flex;gap:8px"><input type="password" style="flex:1;font-family:var(--mono);font-size:12.5px" placeholder="Paste the token" autocomplete="off" spellcheck="false" aria-label="Token">
      <button class="dbtn pri" data-toast="Checked and saved">${label}</button><button class="dbtn quiet" ${dsetGo("keys")}>Cancel</button></div></div>
    ${check ? `<div style="font-size:12px;color:var(--d-muted)">Your machines get it as ${check}</div>` : ""}${html ? note(html) : ""}
    <div class="derr">${err || ""}</div></div>`;
  const ghPage = "github.com/settings/tokens/new";
  const waitCode = `<div style="padding:8px 10px;border-radius:8px;border:1px solid var(--d-border);font-size:12.5px;display:grid;gap:6px"><span>Enter code <code style="font-family:var(--mono);font-size:18px;font-weight:600;letter-spacing:.1em;user-select:all;cursor:text">AB23-77BA</code> on ${lk("github.com/login/device", "GitHub")}<span style="color:var(--d-muted)"> (copied)</span></span><span style="display:flex;gap:6px;align-items:center;color:var(--d-muted)"><span class="dspin"></span>Waiting for GitHub</span></div>`;
  const paste = (quiet, key = "keys-paste") => `<button class="dbtn${quiet ? " quiet" : ""}" ${sm} ${dsetGo(key)}>Paste a token</button>`;
  const ghAccount = (login, orgs, dflt, extra = {}) => {
    const shown = orgs.slice(0, 3).map(o => chip(o)).join(""), more = orgs.slice(3);
    return gitRow({ icon: GH, name: "GitHub", account: `@${login}`, usable: !extra.unreadable, tag: dflt ? `<span class="dgchip" title="GH_TOKEN on your machines">default</span>` : "",
      acts: (extra.use ? `<button class="dbtn pri" ${sm} data-toast="Checked with GitHub and saved">Use gh sign-in</button>` : "") + (dflt ? "" : `<button class="dbtn" ${sm} data-toast="@${login} is now GH_TOKEN on your machines">Make default</button>`) + editRm(`@${login}`, "keys-replace"),
      chips: (extra.unreadable ? chip("Re-enter this token", "", "err") : "") + shown + (more.length ? chip(`+${more.length}`, more.join(", ")) : "") + cliChip("gh", MS) });
  };
  const choice = (login, active) => `<div style="display:flex;align-items:center;gap:8px;font-size:12.5px"><span style="color:var(--d-muted)">gh on this Mac as @${login}</span>${active ? `<span style="font-size:10.5px;color:var(--d-dim)">active</span>` : ""}<span style="margin-left:auto"><button class="dbtn pri" ${sm} data-toast="Checked with GitHub and saved as @${login}">Use</button></span></div>`;
  const credsBody = v => {
    if (v === "keys-load") return skel(2);
    if (v === "keys-fail") return `<div class="dmuted">Could not load your credentials.</div>`;
    const head = (t, top) => `<div style="font-size:11px;font-weight:600;letter-spacing:.06em;text-transform:uppercase;color:var(--d-dim);margin:${top}px 0 2px">${t}</div>`;
    const add = (key, t) => `<button class="dbtn" style="font-size:11.5px;padding:3px 9px" ${dsetGo(key)}>${t}</button>`;
    const cli = { "keys-cli-in": "in", "keys-cli-out": "out", "keys-cli-code": "code", "keys-cli-none": "none", "keys-cli-many": "many", "keys-cli-paste": "paste" }[v];
    let github, canAdd = true;
    if (cli === "paste") { github = credForm({ title: "GitHub", url: "Another account", html: `Make one on ${lk(ghPage, "GitHub")} with repo access, or run ${cd("gh auth login")} in Terminal and use that sign-in instead.` }); canAdd = false; }
    else if (cli) {
      canAdd = false;
      const offer = {
        in: { account: "gh on this Mac as @you", acts: `<button class="dbtn pri" ${sm} data-toast="Checked with GitHub and saved as @you">Use gh sign-in</button>${paste(true, "keys-cli-paste")}` },
        out: { acts: `<button class="dbtn pri" ${sm} ${dsetGo("keys-cli-code")}>Sign in with GitHub</button>${paste(true, "keys-cli-paste")}`, help: "Opens GitHub in your browser, then saves the sign-in here" },
        code: { acts: `<button class="dbtn quiet" ${sm} ${dsetGo("keys-cli-out")}>Cancel</button>`, body: waitCode },
        none: { acts: paste(true, "keys-cli-paste"), help: `Make a token on ${lk(ghPage, "GitHub")}, then paste it.<br>Or install GitHub CLI (${cd("brew install gh")}) and sign in here` },
        many: { account: "gh on this Mac has 2 accounts", acts: paste(true, "keys-cli-paste"), body: `<div style="display:grid;gap:6px">${choice("octo-one", true)}${choice("octo-two")}</div>` },
      }[cli];
      github = gitRow({ icon: GH, name: "GitHub", account: offer.account, usable: false, acts: offer.acts, chips: cliChip("gh", MS), more: (offer.help ? note(offer.help) : "") + (offer.body || "") });
    } else if (v === "keys-gh2" || v === "keys-gh-add" || v === "keys-gh-code") {
      github = ghAccount("octo-one", ["octo-org", "acme", "acme-labs", "widget-co", "infra-team"], true) + ghAccount("octo-two", ["octo-org"], false);
    } else if (v === "keys-unread") {
      github = ghAccount("you", ["octo-org"], true, { unreadable: true, use: true });
    } else if (v === "keys-replace") {
      github = credForm({ title: "GitHub", url: "Replace the token", html: `Make one on ${lk(ghPage, "GitHub")} with repo access, or run ${cd("gh auth login")} in Terminal and use that sign-in instead.` });
    } else {
      github = ghAccount("you", ["octo-org"], true);
    }
    const fj = (name, account, chips, acts) => gitRow({ icon: FJ, name, account, acts: acts || editRm(name, "keys-fjreplace"), chips });
    const fjForm = v === "keys-fjreplace" ? credForm({ title: "git.example.com", url: "Replace the token", html: `Make one at ${lk("git.example.com/user/settings/applications", "git.example.com/user/settings/applications")}, with repository and issue access.` }) : null;
    let hosts = fjForm || fj("git.example.com", "@you", cliChip("teacli", ["build-box", "mac-box"]));
    hosts += fj("code.example.org", "@you", chip("checked from build-box", "The server cannot reach code.example.org; build-box can") + cliChip("teacli", ["build-box"]));
    hosts += fj("forge.example.lan", "", chip("not reachable from the server or your machines", "", "warn") + cliChip("teacli", ["mac-box"]));
    if (v === "keys-fjstates") {
      hosts += fj("git.old.example", "", chip("checking from your machines…") + cliChip("teacli", ["mac-box"]));
      hosts += fj("git.team.example", "", chip("not checked: the server could not reach it", "", "warn") + cliChip("teacli", MS));
      hosts += gitRow({ icon: FJ, name: "git.work.example", account: "@you", acts: editRm("git.work.example", "keys-fjreplace"), chips: chip("token refused", "401 Unauthorized", "err") + cliChip("teacli", MS), more: `<span class="derr">401 Unauthorized</span>` });
    }
    if (cli === "in") hosts += gitRow({ icon: FJ, name: "git.example.net", account: "teacli on this Mac", usable: false, acts: `<button class="dbtn pri" ${sm} data-toast="Checked with git.example.net and saved">Use teacli sign-in</button>`, more: note("Signed in with teacli here, not in Beep yet") });
    const adding = v === "keys-gh-add" || v === "keys-gh-code"
      ? `<div style="padding:10px 0;border-bottom:1px solid var(--d-border);display:grid;gap:6px;font-size:13px"><div style="display:flex;align-items:center;gap:8px"><b style="font-weight:600">Add another GitHub account</b><span style="margin-left:auto"><button class="dbtn quiet" ${sm} ${dsetGo("keys-gh2")}>Cancel</button></span></div>
        <div style="display:grid;gap:6px;padding-left:15px">${v === "keys-gh-code" ? waitCode : `${choice("octo-three", true)}${choice("octo-four")}<div style="display:flex;gap:6px"><button class="dbtn" ${sm} ${dsetGo("keys-gh-code")}>Sign in with GitHub</button>${paste(true, "keys-cli-paste")}</div>${note("Sign in adds the account to gh here too. @octo-one is in Beep already.")}`}</div></div>` : "";
    if (adding) canAdd = false;
    const addingFj = v === "keys-gh";
    const fjNew = addingFj ? credForm({ title: "New Forgejo/Gitea server", url: "one token per server, teacli on every machine", urlField: true, html: `Make one on your server at ${cd("/user/settings/applications")}, with repository and issue access. Or run ${cd("teacli login https://<server> --token <token>")} on this Mac and use that sign-in.` }) : "";
    const adds = (canAdd ? add("keys-gh-add", "+ Add another GitHub account") : "") + (addingFj ? "" : add("keys-gh", "+ Add a Forgejo/Gitea server"));
    return `<div style="display:grid">${head("Git hosts", 4)}${github}${hosts}${adding}${fjNew}${adds ? `<div style="display:flex;flex-wrap:wrap;gap:6px;margin-top:8px">${adds}</div>` : ""}</div>`;
  };

  // Instructions: instructions.ts.
  const insTab = (k, label, cur) => `<button class="dbtn tm-tab"${cur === k ? ' aria-current="true"' : ""} data-dact="dset:ins:${k}">${label}</button>`;
  const insSec = (title, small, body) => `<div><div class="cc-sec"><h4>${title}</h4>${small ? `<small>${small}</small>` : ""}</div>${body}</div>`;
  const inFile = (ag, path, r, miss) => `<div class="in-file${miss ? " miss" : ""}"><span class="ag">${ag}</span><code>${path}</code><span class="r">${r}</span></div>`;
  const inMach = (name, right, rows, bad) => `<div class="in-mach"><b><span class="ddot${bad ? " bad" : ""}"></span>${name}</b>${right || "<span></span>"}<div class="in-files">${rows.join("")}</div></div>`;
  const insBody = () => {
    const v = S.d.ins || "editor";
    const cur = v.startsWith("import") || v === "reading" || v === "nomachine" ? "import" : v.startsWith("sync") ? "sync" : v.startsWith("history") ? "history" : "editor";
    const tabs = `<div class="in-tabs">${insTab("editor", "Editor", cur)}${insTab("import", "Import", cur)}${insTab("sync", "Sync", cur)}${insTab("history", "History", cur)}</div>`;
    const empty = (b, s) => `<div class="cc-panel cc-empty"><b>${b}</b>${s ? `<span>${s}</span>` : ""}</div>`;
    let body;
    if (v === "loading") body = empty("Loading your instructions…");
    else if (cur === "editor") {
      const dirty = v === "editor-dirty", first = v === "editor-first";
      const layers = insSec("How agents see it", "first to last", `<div class="cc-panel"><ol class="in-layers">
        <li><b>The agent's own rules</b><small>Built into Claude Code, Codex, Gemini CLI or pi</small></li>
        <li><b>Your text in its file</b><small>The rest of ~/.claude/CLAUDE.md and the like</small></li>
        <li class="me"><b>Your instructions</b><small>${first ? "Nothing saved yet, so agents get only the other layers" : "Version 7, between Beep's markers in each agent's file. Sync shows which machines have it"}</small></li>
        <li><b>Project</b><small>The repository's CLAUDE.md or AGENTS.md</small></li>
        <li><b>Task</b><small>The prompt Beep starts the agent with</small></li></ol></div>`);
      const files = insSec("Agent files", "", `<div class="cc-panel cc-row in-side"><span class="t"><b>Agent files</b><span>Write your rules into each agent's own file</span></span><button class="dbtn" data-dact="dset:ins:sync">Sync</button></div>`);
      body = `<div class="in-cols">${insSec("For all agents", first ? "Markdown · 0.0 of 16 KB" : "Markdown · 0.6 of 16 KB", `<div class="cc-panel in-ed">
        <div class="in-ver"><span><b>${first ? "Not saved yet" : "Version 7"}</b>${first ? "" : " · 2 days ago"}</span><span class="sp">${dirty ? "Not saved" : first ? "" : "Saved"}</span></div>
        <textarea aria-label="Your instructions" spellcheck="false" placeholder="# How I work&#10;&#10;- Write plainly.&#10;- Ask before deploying anything.">${first ? "" : IN_RULES}${dirty ? "\n- One more rule." : ""}</textarea>
        <div class="in-acts"><button class="dbtn pri"${dirty ? ' data-toast="Saved as version 8"' : " disabled"}>Save</button>${dirty ? '<button class="dbtn" data-dact="dset:ins:editor">Discard</button>' : ""}<button class="dbtn" data-dact="dset:ins:history">History</button><button class="dbtn" data-dact="dset:ins:import">Import from machines</button></div></div>`)}
        <div class="in-sidecol">${layers}${files}</div></div>`;
    } else if (v === "reading") body = empty("Reading the agent files on your machines…");
    else if (v === "nomachine") body = insSec("Found on your machines", "read just now", `<div class="cc-panel"><div class="cc-empty">No machine online</div></div>`) + `<div class="in-acts"><button class="dbtn" data-toast="Read again">Read again</button></div>`;
    else if (cur === "import") {
      const small = t => `<button class="dbtn small" data-toast="${t}">${t}</button>`;
      const has = (lines, show) => `${lines}<button class="dbtn small" data-dact="dset:ins:${show ? "import" : "import-show"}">Show</button><button class="dbtn small" data-dact="dset:ins:editor-dirty">Add to mine</button><button class="dbtn small quiet" data-dact="dset:ins:editor-dirty">Use instead</button>`;
      const open = v === "import-show";
      const machines = inMach("build-box", "", [inFile("Claude Code", "~/.claude/CLAUDE.md", has("241 lines", open)), inFile("Codex", "~/.codex/AGENTS.md", "only Beep's text"), inFile("Gemini CLI", "~/.gemini/GEMINI.md", "none", true), ...(open ? [`<pre class="in-pre"># How I work\n\n- Write plainly.\n- Never push to main.</pre>`] : [])])
        + inMach("mac-box", "", [inFile("Claude Code", "~/.claude/CLAUDE.md", has("198 lines")), inFile("pi", "~/.pi/agent/AGENTS.md", has("30 lines"))])
        + inMach("win-box", "", [`<div class="in-file miss"><span>No coding agent here</span></div>`])
        + inMach("gpu-box", "", [`<div class="in-file miss"><span>gpu-box is offline</span></div>`], true);
      body = insSec("Found on your machines", "read just now", `<div class="cc-panel">${machines}</div>`) + `<div class="in-acts"><button class="dbtn" data-toast="Read again">Read again</button></div>`;
    } else if (cur === "sync") {
      const none = v === "sync-none";
      const st = (t, cls) => cls ? `<span class="in-${cls}">${t}</span>` : `<span>${t}</span>`;
      const write = (off, t = "Write to agent files") => none ? "" : `<button class="dbtn small"${off ? " disabled" : ` data-toast="Wrote version 7"`}>${t}</button>`;
      const f = (ag, p, s, c) => inFile(ag, p, st(s, c));
      const machines = inMach("build-box", write(false), [f("Claude Code", "~/.claude/CLAUDE.md", none ? "Has its own text" : "Synced", none ? "" : "ok"), f("Codex", "~/.codex/AGENTS.md", none ? "Has its own text" : "Not synced", none ? "" : "warn"), f("Gemini CLI", "~/.gemini/GEMINI.md", "Not written yet")])
        + inMach("mac-box", write(true), [f("Claude Code", "~/.claude/CLAUDE.md", none ? "Has its own text" : "Synced", none ? "" : "ok")])
        + inMach("win-box", write(true), [`<div class="in-file miss"><span>No coding agent here</span></div>`]);
      const lead = none ? `<button class="cc-note in-go" data-dact="dset:ins:editor-first"><span class="ddot off"></span><span><b>Nothing saved yet.</b> Save your rules in Editor, then write them here.</span><span class="go">Open Editor</span></button>`
        : `<div class="cc-note"><span class="ddot"></span><span>Written between <code>&lt;!-- beep:start --&gt;</code> and <code>&lt;!-- beep:end --&gt;</code> in each agent's file. Text outside them is never touched.</span></div>`;
      body = lead + insSec("Machines", none ? "nothing saved yet" : "version 7", `<div class="cc-panel">${machines}</div>`) + `<div class="in-acts">${none ? "" : '<button class="dbtn" data-toast="Wrote version 7 to the machines behind">Write to all machines</button>'}<button class="dbtn" data-toast="Read again">Read again</button></div>`;
    } else if (v === "history-none") body = empty("No versions yet", "Each save makes one.");
    else if (v === "history-load") body = empty("Loading the versions…");
    else {
      const open = v === "history-open" ? 6 : 0;
      const h = (n, what, when, newest) => `<div class="in-hist"><div class="h"><b>${what}</b><small>${when}</small><span class="acts"><button class="dbtn small" data-dact="dset:ins:${open === n ? "history" : "history-open"}">${open === n ? "Hide" : "Show"}</button>${newest ? "" : `<button class="dbtn small" data-toast="Restored as version 8">Restore</button>`}</span></div>${open === n ? `<pre class="in-pre">${IN_RULES.split("\n").slice(0, 6).join("\n")}</pre>` : ""}</div>`;
      body = insSec("History", "newest first", `<div class="cc-panel">${h(7, "Version 7", "2 days ago", true)}${h(6, "Version 6 · Imported from Claude Code on build-box", "5 days ago")}${h(5, "Version 5 · Restored version 3", "1 week ago")}${h(4, "Version 4", "2 weeks ago")}</div>`);
    }
    return `<div class="cc-view in-view">${tabs}${body}</div>`;
  };

  // Account: setDevicesHtml and setAccountHtml.
  const accountBody = v => {

    const OUT_NOTE = "The assistant and voice are off. Machines, agents and projects keep working.";
    const bsRow = (t, sub, end, c = "") => dt(t, sub, end);
    if (v === "account-out") return bsRow("Signed out", OUT_NOTE, `<button class="dbtn pri" ${dsetGo("account")} data-toast="Signed in. Memories, routines and chats come back">Sign in</button>`, " off");
    if (v === "account-down") return bsRow("Not reachable", "Beep cannot reach the server. Try again, or sign out.", `<button class="dbtn" data-toast="Trying again">Try again</button><button class="dbtn quiet" ${dsetGo("account-signout")}>Sign out…</button>`, " bad");
    const inRow = bsRow("Signed in", "Chats, memories and routines sync with the server.", v === "account-signout" ? "" : `<button class="dbtn" ${dsetGo("account-signout")}>Sign out…</button>`);
    const signOut = v === "account-signout" ? `<div class="dcard"><div style="padding:10px 12px;display:flex;flex-direction:column;gap:8px"><b style="font-size:13.5px">Sign out of Beep server?</b><div class="dmuted" style="display:block">This Mac stops using the server, so the assistant and voice go off. Signing in brings it all back.</div>
      <div style="display:flex;gap:8px"><button class="dbtn warn" ${dsetGo("account-out")} data-toast="Signed out">Sign out</button><button class="dbtn quiet" ${dsetGo("account")}>Cancel</button></div></div></div>` : "";
    if (v === "account-none") return bsRow("No account", "Adds chats, memories, routines, skills, apps, research, the project agent and phones.", `<button class="dbtn pri" data-dgo="dLink">Sign in</button>`, " off");
    const owner = v === "account" || v === "account-gh" || v === "account-signout";
    const tag = t => `<span class="dtag">${t}</span>`;
    const row = (n, tags, kind, rm) => `<div class="dtoggle"><span><b style="display:flex;gap:7px;align-items:center">${n}${tags.map(tag).join("")}</b><small>${kind}</small></span>${rm ? `<button class="dbtn quiet" aria-label="Remove ${n}" data-toast="${n} removed">${DI.x}</button>` : ""}</div>`;
    if (v === "account-load") return inRow + skel(2);
    const rows = owner ? row("Alex's MacBook Air", ["This Mac", "Owner"], "Computer", false) + row("Pixel", [], "Phone", true)
      : row("Pixel", ["Owner"], "Phone", false) + row("Alex's MacBook Air", ["This Mac"], "Computer", false);
    const ask = v === "account-unlink" ? `<div class="dcard"><div style="padding:10px 12px;display:flex;flex-direction:column;gap:8px"><b style="font-size:13.5px">Unlink this Mac?</b><div class="dmuted" style="display:block">This Mac stops seeing your chats and memories. Your phone keeps them. To use Beep here again, start over and link it from the phone.</div>
      <div style="display:flex;gap:8px"><button class="dbtn warn" ${dsetGo("account-none")} data-toast="Unlinked">Unlink</button><button class="dbtn quiet" ${dsetGo("account-member")}>Cancel</button></div></div></div>` : "";
    const link = v === "account-unlink" ? "" : `<button class="dbtn" data-dgo="dInvite">Link a phone…</button>`;
    const unlink = owner || v === "account-unlink" ? "" : `<div class="dtoggle"><span><small>This Mac: Alex's MacBook Air</small></span><button class="dbtn warn" ${dsetGo("account-unlink")}>Unlink this Mac</button></div>`;
    const devices = `<div class="dsec flat dgroup">Devices${link}</div><div class="dcard ddevices">${rows}${unlink}</div>`;
    return inRow + signOut + devices + ask;
  };

  // A coding agent's page: providers.ts, with its machine picker and summary in Settings.
  const HV = {
    claude: { name: "Claude Code", ver: "2.1.312", latest: "2.1.320", status: "Signed in as you@example.com · Max plan", list: "Signed in · Max plan", home: "CLAUDE_CONFIG_DIR", hdef: "~/.claude", prog: "claude", diffs: { program: ["mac-box", "win-box"], args: ["win-box"] } },
    codex: { name: "Codex", ver: "0.166.0", status: "Signed in as you@example.com · Pro plan", list: "Signed in · Pro plan", home: "CODEX_HOME", hdef: "~/.codex", prog: "codex" },
    gemini: { name: "Gemini CLI", missing: true, install: "npm i -g @google/gemini-cli" },
    pi: { name: "pi", ver: "0.99.2", status: "Installed", list: "Installed", unknown: true, home: "PI_HOME", hdef: "~/.pi", prog: "pi" },
    opencode: { name: "OpenCode", ver: "1.19.4", status: "Not signed in on this machine", list: "Not signed in on this machine", bad: true, out: true, home: "OPENCODE_HOME", hdef: "~/.opencode", prog: "opencode" },
    flsh: { name: "flsh", ver: "0.41.0", status: "Installed", list: "Installed", unknown: true, home: "FLASHMIND_HOME", hdef: "~/.flashmind", prog: "flsh" },
  };
  const hvSwitch = (a, on) => swi(on, `${HV[a].name} on build-box`, `data-toast="${on ? "Turned off on build-box" : "Turned on on build-box"}"`);
  const nameLine = (name, ver, latest) => `<span class="hv-nm"><b>${name}</b>${ver ? `<span class="ver">v${ver}</span>` : ""}${latest ? `<span class="hv-up" title="v${latest} is out">update</span>` : ""}</span>`;
  const statusSmall = (t, bad) => `<small${bad ? ' class="bad"' : ""} title="${t}">${t}</small>`;
  const hvPicker = () => `<span class="hv-pick"><span class="hv-seg" role="group" aria-label="Machine">${[["build-box", 1], ["mac-box", 1], ["win-box", 0]].map(([n, on]) => `<button type="button" title="${on ? n : `${n} (offline)`}" aria-pressed="${n === "build-box"}"${on ? "" : " disabled"} data-toast="Showing ${n}">${n}</button>`).join("")}</span></span>`;
  const hvIn = (val, ph, label) => `<input class="hv-in" value="${val}" placeholder="${ph}" spellcheck="false" autocomplete="off" aria-label="${label}">`;
  const hvField = (label, help, ctl, names = []) => `<div class="hv-f"><span><b>${label}</b>${names.length ? `<span class="hv-diff" title="Not the same on ${names.join(", ")}">differs on ${names.length} machine${names.length === 1 ? "" : "s"}</span>` : ""}<small>${help}</small></span>${ctl}</div>`;
  const hvEnv = () => `<div class="hv-envs"><div class="hv-env">${hvIn("HTTPS_PROXY", "NAME", "Variable name")}<input class="hv-in" type="password" placeholder="saved; type to replace" aria-label="Value"><button class="dbtn quiet small" aria-label="Remove HTTPS_PROXY" data-toast="Removed">&#215;</button></div><div class="hv-env">${hvIn("", "NAME", "Variable name")}<input class="hv-in" type="password" placeholder="value" aria-label="Value"><span></span></div></div>`;
  const hvRuntime = (a, h, saved) => `<h4>Runtime</h4>${hvField("Binary path", `The command Beep starts for ${h.name} on build-box.`, hvIn("", h.prog, "Binary path"), h.diffs?.program)}${hvField(h.home, "Its home and config folder.", hvIn("", h.hdef, h.home))}${hvField("Launch arguments", "Added to every session it starts.", hvIn("", "none", "Launch arguments"), h.diffs?.args)}${a === "flsh" || a === "gemini" ? "" : hvField("Auto-compact", a === "opencode" ? "Compact when the conversation nears the window." : "Compact at this share of the window, or this many tokens.", `<span class="fl-row">${swi(true, `Auto-compact for ${h.name}`)}${a === "opencode" ? "" : hvIn("", "80% or 350000", "Auto-compact threshold")}</span>`)}${hvField("Environment", "Encrypted, never shown again. Put keys in Credentials.", hvEnv())}
    <div class="hv-acts hv-save"><button class="dbtn pri" title="Save for build-box only"${saved === "dirty" ? ' data-toast="Saved for build-box"' : " disabled"}>Save</button><button class="dbtn" title="Save these settings on every machine; offline ones get them when they next connect" data-toast="Saved to 3 machines">Save to all hosts</button><label class="hv-incl" title="Binary paths usually differ between machines"><input type="checkbox"> Include binary path</label>${saved === "dirty" ? '<button class="dbtn quiet">Discard</button>' : ""}<span class="hv-ok">${saved === "all" ? "Saved to 3 machines" : ""}</span></div>${["codex", "claude", "pi"].includes(a) ? hvModels() : ""}`;
  const hvModels = () => `<h4>Models and context</h4><div class="hv-note">Limits from this machine's harness. Maximum usable can be smaller than the API limit.</div><div class="hv-note">Overrides apply to new or resumed sessions on every machine, between turns. Blank uses Auto-compact above; off disables Beep's trigger. The harness may compact earlier. Enter 80%, 350000, or off.</div>
    <input class="hv-in" placeholder="Filter models" aria-label="Filter models"><div class="model-context-scroll"><table class="fl-tbl model-context"><thead><tr><th>Model</th><th>Default usable</th><th>Maximum usable</th><th>Compact at</th></tr></thead><tbody>
    <tr><td>model-large</td><td>360,000</td><td>720,000</td><td>${hvIn("350000", "Harness setting", "Compact at for model-large")}</td></tr>
    <tr><td>model-small</td><td>Not reported</td><td>Not reported</td><td>${hvIn("", "Harness setting", "Compact at for model-small")}</td></tr></tbody></table></div><div class="hv-acts"><button class="dbtn small" data-toast="Added">Add model</button><button class="dbtn pri" data-toast="Model overrides saved">Save model overrides</button><button class="dbtn quiet" data-toast="Changes discarded">Discard</button></div>`;
  const flshOwn = () => {
    const tri = (opts, on, label) => `<span class="fl-seg" role="group" aria-label="${label}">${opts.map(o => `<button class="dchip" aria-pressed="${o === on}">${o}</button>`).join("")}</span>`;
    const quants = `<span class="fl-chips" role="group" aria-label="Allowed quantizations">${["int4", "int8", "fp4", "fp6", "fp8", "fp16", "bf16", "fp32", "unknown"].map(q => `<button class="dchip" aria-pressed="${["fp8", "fp16", "bf16"].includes(q)}">${q}</button>`).join("")}</span>`;
    const models = [["openrouter:vendor/model-large", true, "300000"], ["openrouter:vendor/model-flash", true, ""], ["venice:model-uncensored", false, ""]];
    const DEF = "Model's default trigger";
    return `<h4>OpenRouter routing</h4><div class="hv-note">Applies to every machine.</div>
      ${hvField("Zero data retention", "Only endpoints that keep no prompts.", tri(["Account setting", "Require", "Off"], "Require", "Zero data retention"))}
      ${hvField("Allowed quantizations", "Weight precisions providers may serve.", quants)}
      ${hvField("Sort providers by", "Default: OpenRouter balances price and uptime.", tri(["Default", "Price", "Throughput", "Latency"], "Throughput", "Sort providers by"))}
      ${hvField("Allow fallbacks", "Try other providers when the first ones fail.", tri(["On (OpenRouter default)", "Off"], "On (OpenRouter default)", "Allow fallbacks"))}
      <h4>Auto-compact</h4>
      ${hvField("All models", "Compact once the prompt passes this many tokens.", `<span class="fl-row">${swi(true, "Auto-compact for all models")}${hvIn("", DEF, "Auto-compact threshold")}</span>`)}
      <table class="fl-tbl"><tr><th>Model</th><th>On</th><th>Threshold (tokens)</th><th></th></tr>
      ${models.map(([m, on, t], i) => `<tr><td>${hvIn(m, "provider:model", `Model ${i + 1}`)}</td><td>${swi(on, `Auto-compact for model ${i + 1}`)}</td><td>${hvIn(t, on ? DEF : "Off", `Threshold for model ${i + 1}`)}</td><td><button class="dbtn quiet small" aria-label="Remove ${m}" data-toast="Removed ${m}">&#215;</button></td></tr>`).join("")}
      <tr><td></td><td></td><td></td><td><button class="dbtn small" data-toast="Added">Add model</button></td></tr></table>
      <div class="hv-acts"><button class="dbtn pri" disabled>Save</button></div>`;
  };
  const hvStatusBlock = (a, h, inSet, st) => {
    const m = "build-box", row = html => inSet ? "" : `<div class="hv-stat">${html}</div>`;
    if (h.missing) return row(`Not installed on ${m}.`) + `<div class="hv-acts"><button class="dbtn pri" data-toast="Installing ${h.name} on ${m}">Install on ${m}</button><button class="dbtn quiet" data-toast="Copied: ${h.install}">Copy install command</button></div>`;
    let out = "";
    if (st === "token") out += row(`<span class="ddot"></span>Signs in with the token in Agent accounts (…a91f)`) + `<div class="hv-acts"><button class="dbtn" ${dsetGo("acct")}>Agent accounts</button></div>`;
    else if (st === "out" || h.out) {
      out += row(`<span class="ddot" style="background:var(--d-coral)"></span>Not signed in on ${m}`);
      out += a === "claude" || a === "codex" ? `<div class="hv-acts"><button class="dbtn pri" ${dsetGo("acct")}>Sign in</button></div>` : `<div class="hv-note">Run ${a} on ${m} and sign in there.</div>`;
    } else if (h.unknown) {
      const why = `${h.name} does not say whether it is signed in. Run it on ${m} to sign in.`;
      out += inSet ? `<div class="hv-note">${why}</div>` : row(why);
    } else out += row(`<span class="ddot"></span>${h.status}`);
    // Its ACP adapter, which beep-node installs itself (#425).
    if (st === "adapting") out += `<div class="hv-note">Installing the ${h.name} adapter</div>`;
    else if (st === "adapter") out += `<div class="hv-note bad">Could not install the ${h.name} adapter: could not download Node.js: no curl or wget</div><div class="hv-acts"><button class="dbtn" data-toast="Trying again on ${m}">Try again</button></div>`;
    else if (st === "stale") out += `<div class="hv-note">Restart beep-node to use it.</div>`;
    if (h.latest) out += `<div class="hv-acts"><button class="dbtn"${st === "updating" ? " disabled" : ` data-toast="Updating ${h.name} on ${m}"`}>${st === "updating" ? "Updating…" : `Update to v${h.latest}`}</button></div>`;
    return out;
  };
  const failLine = (t, more) => `<div class="fb-err" role="alert"><div class="fb-line"><span>${t}</span><button class="fb-x" aria-label="Dismiss" data-toast="Dismissed">&#215;</button></div>${more ? `<details class="fb-more"><summary>Details</summary><pre>${more}</pre></details>` : ""}</div>`;
  // A coding agent's page in Settings. `k` is the agent, then its state: acct-claude, acct-claude-off.
  const harnessPage = v => {
    const [, a, st = ""] = v.split("-");
    const bar = `<div class="hv-bar"><span class="lbl">Machine</span>${hvPicker()}<span class="sp"></span><button class="dbtn quiet" title="Check the versions again" data-toast="Checked build-box again">Checked just now</button><button class="dbtn"${a === "claude" ? ' data-toast="Updating 1"' : ' disabled title="Everything is up to date"'}>Update all</button></div>`;
    const wrap = inner => `<div class="cc-view hv-view in-set">${bar}${inner}</div>`;
    if (st === "none") return `<div class="cc-view hv-view in-set"><div class="hv-bar"><span class="lbl">Machine</span>${hvPicker().replace(/aria-pressed="true"/, 'aria-pressed="false"')}<span class="sp"></span></div><div class="cc-panel cc-empty"><b>No machine online</b><span>Coding agents show here for machines that are online.</span></div></div>`;
    if (st === "asking") return wrap(`<div class="cc-panel cc-empty"><b>Asking build-box…</b></div>`);
    if (st === "old") return wrap(`<div class="cc-err">The server needs an update before it shows coding agents here.</div>`);
    if (st === "gone") return wrap(`<div class="cc-panel cc-empty"><b>build-box does not report ${HV[a].name}</b></div>`);
    const h = HV[a], off = st === "off";
    const on = !h.missing && !off;
    let status = h.status, bad = h.bad;
    if (h.missing) status = "Not installed";
    if (off) status = "Turned off: Auto and the start form skip it";
    if (st === "token") status = "Signs in with Beep's token …a91f";
    if (st === "out") { status = "Not signed in on this machine"; bad = true; }
    if (st === "adapting") status = `Installing the ${h.name} adapter`;
    if (st === "adapter") { status = `Could not install the ${h.name} adapter`; bad = true; }
    const end = h.missing ? "<span></span><span></span>" : `<span class="on">${on ? "On" : "Off"} for build-box</span>${hvSwitch(a, on)}`;
    const summary = `<div class="hv-sum${!h.missing && !on ? " off" : ""}">${tile(a)}<span class="hv-name">${nameLine(h.name, h.ver, h.latest)}${statusSmall(status, bad)}</span>${end}</div>`;
    const banner = st === "fail" ? failLine("Could not update Claude Code on build-box: npm exited with code 1", "$ npm i -g @anthropic-ai/claude-code\nnpm ERR! code EACCES") : "";
    const detail = `<div class="hv-d">${hvStatusBlock(a, h, true, st)}${h.missing ? "" : hvRuntime(a, h, st === "dirty" ? "dirty" : st === "saved" ? "all" : "")}${a === "flsh" ? flshOwn() : ""}</div>`;
    return wrap(banner + summary + detail);
  };
  // The window's own Coding agents view: the list on the left, the picked agent's page on the right.
  const providers = (v, failBanner) => {
    const sel = v === "claude" || v === "signin" || v === "token" ? "claude" : v === "missing" ? "gemini" : v === "off" ? "pi" : v === "opencode" ? "opencode" : v === "flsh" ? "flsh" : "codex";
    const st = v === "signin" ? "out" : v === "token" ? "token" : "";
    const rows = ["claude", "codex", "gemini", "pi", "opencode", "flsh"].map(a => {
      const h = HV[a], isOff = v === "off" && a === "pi", on = !h.missing && !isOff;
      let status = h.missing ? "Not installed" : isOff ? "Turned off: Auto and the start form skip it" : a === "claude" && v === "signin" ? "Not signed in on this machine" : a === "claude" && v === "token" ? "Signs in with Beep's token …a91f" : h.list;
      const bad = h.bad || (a === "claude" && v === "signin");
      return `<div class="hv-it${h.missing || on ? "" : " off"}" role="button" tabindex="0" aria-current="${a === sel}" data-dact="dset:pv:${a === "gemini" ? "missing" : a === "pi" ? "off" : a}">${tile(a)}<span class="hv-name">${nameLine(h.name, h.ver, h.latest)}${statusSmall(status, bad)}</span>${h.missing ? "<span></span>" : hvSwitch(a, on)}</div>`;
    }).join("");
    const h = HV[sel];
    const detail = `<div class="hv-d"><h3>${tile(sel)}${h.name}<span class="ver">${h.missing ? "" : h.latest ? `v${h.ver} · v${h.latest} out` : `v${h.ver}`}</span></h3><h4>Status</h4>${hvStatusBlock(sel, h, false, st)}${h.missing ? "" : hvRuntime(sel, h, "")}${sel === "flsh" ? flshOwn() : ""}</div>`;
    const updBtn = v === "update" ? `<button class="dbtn" disabled>Updating 2…</button>` : `<button class="dbtn" data-dact="dset:pv:update">Update all</button>`;
    const check = `<button class="dbtn quiet" title="Check the versions again" data-toast="Checked build-box again">Checked 4 min ago</button>`;
    const head = `<div class="cc-top"><h2>Coding agents</h2><small>What each machine can run</small><span class="tools">${hvPicker()}${updBtn}${check}</span></div>`;
    const body = v === "none" ? `<div class="cc-panel cc-empty"><b>No machine online</b><span>Coding agents show here for machines that are online.</span></div>`
      : `${v === "fail" ? failBanner : ""}<div class="hv"><div class="hv-list">${rows}</div>${detail}</div>`;
    return ccWin(ccSide(""), `${head}<div class="cc-view hv-view">${body}</div>`);
  };

  // What the left pane lists, and which page a variant shows.
  const PAGES = [["General", "general"], ["Appearance", "appearance"], ["Models", "models"], ["Apps", "apps"], ["This Mac", "mac"], ["Machines", "machines"], ["Agent accounts", "acct"], ["Credentials", "keys"], ["Instructions", "instr"], ["Beep server", "account"]];
  const AGENT_SUBS = CODING.map(([a]) => a);
  const agentOf = v => { const a = v.split("-")[1]; return v.startsWith("acct-") && AGENT_SUBS.includes(a) ? a : null; };
  const known = v => {
    if (!v) return "general";
    if (["owner", "linked", "unlink", "none"].includes(v)) return "general";
    return v;
  };
  const pageOf = v => v === "loading" ? "models" : v.startsWith("acct") ? "acct" : v.startsWith("keys") ? "keys" : v.startsWith("app-") || v.startsWith("mcp-") || v === "apps" ? "apps" : v.startsWith("mach") ? "machines" : v.startsWith("account") ? "account" : v.startsWith("models") ? "models" : v.startsWith("mac") ? "mac" : v;
  const subOf = v => v === "app-gmail" ? "Gmail" : v === "app-fastmail" ? "Fastmail" : v === "mcp-wiki" ? "Team wiki" : v === "mach-build" ? "build-box" : agentOf(v) ? HV[agentOf(v)].name : null;
  const kids = p => p === "apps" ? [["Gmail", "app-gmail"], ["Google Calendar", "app-gmail"]]
    : p === "machines" ? ["mac-box", "laptop", "build-box", "win-box"].map(n => [n, "mach-build"])
    : p === "acct" ? CODING.map(([a, n]) => [n, `acct-${a}`]) : [];
  const pageKey = k => k === "machines" ? "machines-list" : k;
  const nav = v => {
    const p = pageOf(v), sub = subOf(v);
    const shown = S.d.noServer ? PAGES.filter(([, k]) => ["general", "appearance", "mac", "machines", "acct", "keys", "instr"].includes(k)).map(([l, k]) => [k === "acct" ? "Coding agents" : l, k]) : v === "account-none" ? PAGES.filter(([, k]) => ["general", "appearance", "account"].includes(k))
      : v === "account-out" ? PAGES.filter(([, k]) => ["general", "appearance", "machines", "keys", "instr", "account"].includes(k)) : PAGES;
    return shown.map(([l, k]) => `<button class="cc-si" ${dsetGo(pageKey(k))}${p === k && !sub ? ' aria-current="true"' : ""}><span class="tt">${l}</span>${v !== "account-none" && k === "apps" ? '<span class="n">2</span>' : v !== "account-none" && k === "machines" ? '<span class="n">4</span>' : ""}</button>`
      + (p === k ? kids(k).map(([kl, kv]) => `<button class="cc-si sub" ${dsetGo(kv)}${sub === kl ? ' aria-current="true"' : ""}><span class="tt">${kl}</span></button>`).join("") : "")).join("");
  };
  const BACK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M15 6l-6 6 6 6"/></svg>';
  const crumbs = v => {
    const p = pageOf(v), sub = subOf(v), title = S.d.noServer && p === "acct" ? "Coding agents" : PAGES.find(([, k]) => k === p)?.[0] || "General";
    if (!sub) return `<b>${title}</b>`;
    return `<button class="dset-back" ${dsetGo(pageKey(p))} aria-label="Back to ${title}">${BACK}</button><button class="dset-crumb" ${dsetGo(pageKey(p))}>${title}</button>${chev}<b>${sub}</b>`;
  };
  const host = inner => `<section><div class="dset-host">${inner}</div></section>`;
  const page = (v, machPages) => {
    const p = pageOf(v);
    if (machPages[v]) return `<section style="max-width:none">${machPages[v]()}</section>`;
    if (v === "machines") return host(`<div class="dhead"><span class="tt"><b>No machines linked</b></span></div><div class="dmuted" style="display:block;margin-bottom:10px">To add one, use Add a machine: it gives you a one-time command to run there.</div><button class="dbtn" data-dgo="dAddMachine">Add a machine</button>`);
    if (v === "loading") return sec("Models", skel(2));
    if (v === "models-fail") return sec("Models", `<div class="dmuted">Could not load your models.</div>`);
    if (p === "general") return sec("General", generalBody());
    if (p === "appearance") return sec("Appearance", skinsHtml());
    if (p === "models") return sec("Models", modelsBody());
    if (v === "app-gmail") return host(appPerm("", dsetGo("apps")));
    if (v === "app-fastmail") return host(appPerm(S.d.fm || "two", dsetGo("apps")));
    if (v === "mcp-wiki") return host(mcpPage(dsetGo("apps")));
    if (p === "apps") return host(appsList(true));
    if (p === "mac") return `<section>${macBody()}</section>`;
    if (agentOf(v)) return host(harnessPage(v));
    if (p === "acct") return sec(S.d.noServer ? "Coding agents" : "Agent accounts", (S.d.noServer ? "" : acctBody(v)) + codingAgents());
    if (p === "keys") return sec("Credentials", credsBody(v));
    if (p === "instr") return host(insBody());
    if (p === "account") return sec("Beep server", accountBody(v));
    return sec("General", generalBody());
  };
  const frame = (v0, machPages) => {
    const v = known(v0);
    return `<div class="cc-set-scrim" data-dgo="dWinChat"><div class="cc-set" role="dialog" aria-modal="true" aria-label="Settings" data-dgo="dWinSettings">
      <nav class="dset-nav" aria-label="Settings pages"><b>Settings</b><div class="dset-pages">${nav(v)}</div><small class="dset-ver">Beep for macOS 0.2.0</small></nav>
      <div class="dset-body"><div class="dset-top"><span class="dset-crumbs">${crumbs(v)}</span><kbd>esc</kbd></div><div class="dset-page dform">${page(v, machPages)}</div></div></div></div>`;
  };
  // Settings in the bar: every section in one column (settingsSections with sheet=false).
  const barColumn = st => {
    const none = st === "none", so = st === "out";
    const out = [sec("General", generalBody()), sec("Appearance", skinsHtml())];
    if (!none && !so) {
      out.push(sec("Models", modelsBody()));
      out.push(sec("Apps", dt("Connected apps", "2 connected: Gmail, Google Calendar", '<button class="dbtn" data-dgo="dApps">Manage…</button>')));
      out.push(`<section>${macBody()}</section>`);
      out.push(sec("Machines", dt("Other machines", "2 linked: build-box, gpu-box", '<button class="dbtn" data-dgo="dMachines">Show…</button>')));
      out.push(sec("Agent accounts", dt("Coding agents on each machine", "Versions and updates, on or off, binary paths, launch arguments and env vars for Claude Code, Codex, pi and the rest", '<button class="dbtn" data-dgo="dProviders">Open…</button>') + acctBody("acct")));
      out.push(sec("Credentials", credsBody("keys")));
      out.push(sec("Instructions", dt("Your rules for every agent", "One text for Claude Code, Codex, Gemini CLI and pi, with its versions, written into each agent's file on your machines", '<button class="dbtn" data-dgo="dWinInstructions">Open…</button>')));
    }
    if (so) {
      out.push(sec("Machines", dt("Other machines", "2 linked: build-box, gpu-box", '<button class="dbtn" data-dgo="dMachines">Show…</button>')));
      out.push(sec("Credentials", credsBody("keys")));
      out.push(sec("Instructions", dt("Your rules for every agent", "One text for Claude Code, Codex, Gemini CLI and pi, with its versions, written into each agent's file on your machines", '<button class="dbtn" data-dgo="dWinInstructions">Open…</button>')));
    }
    const acc = none ? "account-none" : so ? "account-out" : st === "down" ? "account-down" : st === "signout" ? "account-signout" : st === "owner" ? "account" : st === "unlink" ? "account-unlink" : "account-member";
    out.push(sec("Beep server", accountBody(acc)));
    return `<div class="dform dsetcol" style="padding-top:12px">${out.join("")}</div>`;
  };
  return { frame, barColumn, appsList, appPerm, mcpPage, insBody, providers, generalBody };
})();

// The bar's Settings, Apps, an app and an MCP server.
D.dSettings = () => deskScene(dBar(`<div class="dhead"><span class="tt"><b>Settings</b><small>Beep for macOS 0.2.0</small></span></div>${SETV.barColumn(S.d.set)}` + dHints([["esc", "close"]])));
D.dLiveSettings = () => deskScene(dBar(`<div class="dhead"><span class="tt"><b>Settings</b><small>Beep for macOS 0.2.0</small></span></div><div class="dform dsetcol" style="padding-top:12px"><section><div class="dsec flat" style="padding-top:6px">General</div>${SETV.generalBody()}</section></div>` + dHints([["esc", "close"]])));
D.dApps = () => deskScene(dBar(`<div class="dhead"><span class="tt"><b>Apps</b><small>Connect once, then just ask from any chat</small></span></div>${SETV.appsList(false).replace('class="dlist dapps"', 'class="dlist dapps dsetcol"')}` + dHints([["esc", "back"]])));
D.dAppPerm = () => deskScene(dBar(SETV.appPerm(S.d.fm || "", 'data-dgo="dApps"').replace('class="dlist dapps"', 'class="dlist dapps dsetcol"') + dHints([["esc", "back"]])));
D.dMcp = () => deskScene(dBar(SETV.mcpPage('data-dgo="dApps"').replace('class="dlist dapps"', 'class="dlist dapps dsetcol"') + dHints([["esc", "back"]])));
VARIANTS.dApps = [["List", "list"], ["Nothing connected", "none"], ["Connecting", "waiting"], ["No API key", "nokey"], ["Adding a server", "mcpAdd"], ["Server URL refused", "mcpBad"], ["Server sign-in", "mcpWait"], ["Server not connected", "mcpDown"]].map(([l, k]) => [l, () => { S.d.apps = k; render(); }, () => (S.d.apps || "list") === k]);
VARIANTS.dMcp.push(["Remove, asking", () => { S.d.mcpRm = "1"; render(); }, () => !!S.d.mcpRm]);

// Which states the sheet can show. Apps and the machine page reuse their own state keys.
{
  const set0 = { apps: "list", fm: "", mup: "", ins: "editor", setAuto: "on", noServer: false };
  const rows = [["General", "general"], ["Appearance", "appearance"], ["Models, Auto on", "models"], ["Models, Auto off", "models", { setAuto: "off" }], ["Models, loading", "loading"], ["Models, could not load", "models-fail"],
    ["Apps", "apps"], ["Apps, connecting", "apps", { apps: "waiting" }], ["Apps, adding a server", "apps", { apps: "mcpAdd" }], ["Apps > Gmail", "app-gmail"], ["Apps > Fastmail", "app-fastmail", { fm: "two" }], ["Apps > Team wiki", "mcp-wiki"],
    ["This Mac, off", "mac", { mac: "off" }], ["This Mac, connecting", "mac", { mac: "connecting" }], ["This Mac, failed", "mac", { mac: "failed" }], ["This Mac, allow both", "mac", { mac: "perm" }], ["This Mac, ready", "mac", { mac: "ready" }],
    ["Machines, none", "machines"], ["Machines", "machines-list"], ["Machines > build-box", "mach-build"], ["Machines > build-box, Updating", "mach-build", { mup: "updating" }], ["Machines > build-box, Restarting", "mach-build", { mup: "restarting" }], ["Machines > build-box, waits for sessions", "mach-build", { mup: "waiting" }], ["Machines > build-box, update failed", "mach-build", { mup: "failed" }], ["Machines > build-box, Projects in", "mach-build", { mup: "search" }], ["Machines > build-box, Remove", "mach-build", { mup: "asking" }], ["Machines > build-box, Log", "mach-build", { mup: "log" }], ["Machines > build-box, Log reading", "mach-build", { mup: "log-reading" }], ["Machines > build-box, no log", "mach-build", { mup: "log-none" }], ["Machines > build-box, Log empty", "mach-build", { mup: "log-empty" }], ["Machines > build-box, Log on an old node", "mach-build", { mup: "log-old" }], ["Machines > build-box, Sign in starting", "mach-build", { mup: "signin-starting" }], ["Machines > build-box, Sign in with a device code", "mach-build", { mup: "signin-opencode" }], ["Machines > build-box, Sign in with a pasted code", "mach-build", { mup: "signin-claude" }], ["Machines > build-box, Sign in failed", "mach-build", { mup: "signin-failed" }], ["Machines > build-box, Signed in", "mach-build", { mup: "signin-done" }],
    ["Coding agents, no server", "acct", { noServer: true }], ["Agent accounts", "acct"], ["Agent accounts, Claude not signed in", "acct-none"], ["Agent accounts, approve in browser", "acct-flow"], ["Agent accounts, pasting a token", "acct-paste"], ["Agent accounts, token unreadable", "acct-unread"], ["Agent accounts, Codex device code", "acct-code"], ["Agent accounts, Codex starting", "acct-start"], ["Agent accounts, Codex sign-in failed", "acct-signfail"], ["Agent accounts, Codex API key", "acct-apikey"], ["Agent accounts, typing a key", "acct-keyform"], ["Agent accounts, could not load", "acct-fail"], ["Agent accounts, server too old", "acct-old"],
    ["Agent accounts > Claude Code", "acct-claude"], ["Agent accounts > Claude Code, Beep's token", "acct-claude-token"], ["Agent accounts > Claude Code, not signed in", "acct-claude-out"], ["Agent accounts > Claude Code, turned off", "acct-claude-off"], ["Agent accounts > Claude Code, updating", "acct-claude-updating"], ["Agent accounts > Claude Code, update failed", "acct-claude-fail"], ["Agent accounts > Claude Code, edited", "acct-claude-dirty"], ["Agent accounts > Claude Code, saved to all", "acct-claude-saved"], ["Agent accounts > Claude Code, restart node", "acct-claude-stale"], ["Agent accounts > Codex, installing its adapter", "acct-codex-adapting"], ["Agent accounts > Codex, adapter failed", "acct-codex-adapter"], ["Agent accounts > Claude Code, no machine", "acct-claude-none"], ["Agent accounts > Claude Code, asking", "acct-claude-asking"], ["Agent accounts > Claude Code, server too old", "acct-claude-old"],
    ["Agent accounts > Codex", "acct-codex"], ["Agent accounts > Gemini CLI, not installed", "acct-gemini"], ["Agent accounts > pi", "acct-pi"], ["Agent accounts > OpenCode, not signed in", "acct-opencode"], ["Agent accounts > OpenCode, not reported", "acct-opencode-gone"], ["Agent accounts > flsh", "acct-flsh"],
    ["Credentials", "keys"], ["Credentials, adding Forgejo", "keys-gh"], ["Credentials, Forgejo checks", "keys-fjstates"], ["Credentials, replacing Forgejo token", "keys-fjreplace"], ["Credentials, replacing GitHub token", "keys-replace"], ["Credentials, token unreadable", "keys-unread"], ["Credentials, gh signed in", "keys-cli-in"], ["Credentials, gh signed out", "keys-cli-out"], ["Credentials, GitHub code", "keys-cli-code"], ["Credentials, no gh", "keys-cli-none"], ["Credentials, gh has two accounts", "keys-cli-many"], ["Credentials, pasting a GitHub token", "keys-cli-paste"], ["Credentials, two GitHub accounts", "keys-gh2"], ["Credentials, adding a GitHub account", "keys-gh-add"], ["Credentials, adding one: code", "keys-gh-code"], ["Credentials, loading", "keys-load"], ["Credentials, could not load", "keys-fail"],
    ["Instructions", "instr"], ["Instructions, Import", "instr", { ins: "import" }], ["Instructions, Sync", "instr", { ins: "sync" }], ["Instructions, History", "instr", { ins: "history" }],
    ["Beep server", "account"], ["Beep server, signing out", "account-signout"], ["Beep server, signed out", "account-out"], ["Beep server, not reachable", "account-down"], ["Beep server, linked", "account-member"], ["Beep server, unlinking", "account-unlink"], ["Beep server, loading", "account-load"], ["Beep server, no account", "account-none"]];
  VARIANTS.dWinSettings = rows.map(([l, k, x]) => [l, () => { Object.assign(S.d, set0, { set: k }, x); render(); }, () => (S.d.set || "general") === k && Object.entries(x || {}).every(([a, b]) => S.d[a] === b)]);
  // These two jump to a Settings page.
  VARIANTS.dAgentAccounts = [["Signed in", "acct"], ["Claude not signed in", "acct-none"], ["Codex device code", "acct-code"]].map(([l, k]) => [l, () => { S.d.acc = k; render(); }, () => (S.d.acc || "acct") === k]);
  VARIANTS.dWinInstructions = [["Editor", "editor"], ["Editor, not saved", "editor-dirty"], ["Editor, nothing saved", "editor-first"], ["Loading", "loading"], ["Import", "import"], ["Import, a file shown", "import-show"], ["Import, reading", "reading"], ["Import, no machine", "nomachine"], ["Sync", "sync"], ["Sync, nothing saved", "sync-none"], ["History", "history"], ["History, a version shown", "history-open"], ["History, loading", "history-load"], ["History, none", "history-none"]]
    .map(([l, k]) => [l, () => { S.d.ins = k; render(); }, () => (S.d.ins || "editor") === k]);
  VARIANTS.dProviders = [["Codex", "codex"], ["Claude Code", "claude"], ["Claude not signed in", "signin"], ["Beep's token", "token"], ["Gemini not installed", "missing"], ["pi turned off", "off"], ["OpenCode", "opencode"], ["flsh", "flsh"], ["Updating", "update"], ["Update failed", "fail"], ["No machine online", "none"]]
    .map(([l, k]) => [l, () => { S.d.pv = k; render(); }, () => (S.d.pv || "codex") === k]);
}
