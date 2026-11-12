// ---- Desktop: Command center (CommandCenterWindow, Home) ----
// The full window behind the bar: every linked machine, the sessions running on them and what needs an
// answer. Variants: no machines and one machine offline.
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
.mac .cc-main { --colw: clamp(720px, 60cqw, 960px) }
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
.mac .cc-ag.claude { color: #F2A27A } .mac .cc-ag.codex { color: var(--d-fg) }
.mac .cc-ag.beep { color: var(--d-muted) }
.mac .cc-tag { display: inline-flex; align-items: center; gap: 6px; font-size: 10.5px; font-weight: 500; padding: 0 7px; line-height: 17px; border-radius: 999px; border: 1px solid var(--d-border); color: var(--d-muted); white-space: nowrap }
.mac .cc-tag.run { color: var(--d-emerald); border-color: rgba(94,106,210,.45) }
.mac .cc-tag.wait { color: #F2C94C; border-color: rgba(242,201,76,.45) }
.mac .cc-tag.term { color: var(--d-fg) }
.mac .cc-tag.bad { color: var(--d-err); border-color: rgba(235,87,87,.45) }
.mac .cc-tag .dspin { width: 9px; height: 9px }
.mac .cc-note { display: flex; gap: 10px; align-items: center; padding: 10px 12px; border-radius: 8px; border: 1px solid var(--d-border); background: var(--d-elev); font-size: 12.5px; line-height: 18px; color: rgba(227,228,230,.9) }
.mac .cc-note .ddot { margin-top: 1px }
.mac .cc-empty { padding: 22px 18px; display: grid; justify-items: center; gap: 8px; text-align: center; font-size: 13px; line-height: 19px; color: var(--d-muted) }
.mac .cc-empty b { color: var(--d-fg); font-size: 14px; font-weight: 500 }
.mac .cc-empty ol { margin: 0; padding-left: 18px; text-align: left; display: grid; gap: 3px; color: rgba(227,228,230,.86) }
.mac .cc-empty code { font-size: 12px; background: var(--d-elev); border: 1px solid var(--d-border); border-radius: 4px; padding: 0 4px }
/* The input bars stay at most 760px wide, centered; narrower windows keep the 16px gutter. */
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
}`;
  document.head.append(st);
})();

const CC_IC = {
  home: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 11l9-7 9 7v9a1 1 0 01-1 1h-5v-6H9v6H4a1 1 0 01-1-1z"/></svg>',
  run: '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="12" cy="12" r="9"/><path d="M10 8l6 4-6 4z" fill="currentColor"/></svg>',
};
VARIANTS.dHome = [];
DESK_SCREENS.splice(DESK_SCREENS.findIndex(g => g.group === "Machines"), 0, { group: "Command center", items: [["dHome", "Home", "CommandCenterWindow, Home"]] });
