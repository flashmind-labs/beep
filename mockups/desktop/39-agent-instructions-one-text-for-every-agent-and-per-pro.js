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
.mac .in-diff .add { background: rgba(94,106,210,.16); color: #9FE3C6 } .mac .in-diff .add::before { content: "+"; color: #4CB782 }
.mac .in-diff .del { background: rgba(235,87,87,.14); color: #F4AFA5 } .mac .in-diff .del::before { content: "-"; color: var(--d-err) }
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
.mac .dbtn[disabled] { opacity: .45; cursor: default }
.mac .rv-ver.wait { color: var(--d-muted) } .mac .rv-ver .dspin { width: 9px; height: 9px }
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
D.dWinInstructions = () => {
  const side = ccSidePj("");
  const head = `<div class="cc-top"><h2>Instructions</h2><small>Your rules for every agent Beep runs</small><span class="tools"><button class="dbtn quiet small" data-dgo="dWinSettings">Settings</button></span></div>`;
  return ccWin(side, head + SETV.insBody());
};


// Project board styles: the chips and ages the project page shares.
{
  const st = document.createElement("style");
  st.textContent = `
  .mac .pb-grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 12px }
  /* Three tiles sit three a row, so none is left alone; one column on a narrow window. */
  .mac .pb-grid.n3 { grid-template-columns: repeat(3, minmax(0, 1fr)) }
  @container (max-width: 700px) { .mac .pb-grid.n3 { grid-template-columns: minmax(0, 1fr) } }
  /* How fresh a section's kept data is: its age, or a spinner while it reads again. */
  .mac .sw-age { font-size: 11px; color: var(--d-muted); font-weight: 400; white-space: nowrap }
  .mac .pb-chip { flex: none; font-size: 10.5px; padding: 1px 6px; border-radius: 999px; border: 1px solid var(--d-border); color: var(--d-muted) }
  .mac .pb-chip.run { color: #8A9CF0; border-color: rgba(76,183,130,.4) }
  .mac .pb-chip.wait { color: #F2C94C; border-color: rgba(242,201,76,.45) }
  .mac .pb-chip.bad { color: #F08A7E; border-color: rgba(242,166,95,.45) }
  `;
  document.head.append(st);
}
DESK_SCREENS.find(g => g.group === "Command center").items.push(
  ["dWinInstructions", "Instructions", "CommandCenterWindow, InstructionsView"]);
