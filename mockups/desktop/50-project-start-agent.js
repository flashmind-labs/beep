// Returning from Home restores the session pane and its running terminal tabs.

// No project-agent chat in front: the bar under a project page starts a coding agent on a machine,
// in the project's checkout as it is. Beep makes no branch: the agent is told to make its own branch and
// worktree under .worktrees/ before it changes files. Work on it and Continue still pick a worktree.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .pjc { border-top: 1px solid var(--d-border); padding: 10px 14px 12px; display: grid; gap: 8px }
/* The bar's box and note stay at most 760px wide, centered. */
.mac .pjc > * { width: 100%; max-width: 760px; justify-self: center; box-sizing: border-box }
.mac .pjc .box { position: relative; border: 1px solid var(--d-border); border-radius: 12px; background: var(--d-elev); padding: 10px 12px; display: grid; gap: 8px }
.mac .pjc .ph { color: var(--d-muted); font-size: 13px }
.mac .pjc .ta { all: unset; display: block; field-sizing: content; min-height: 20px; max-height: 180px; overflow-y: auto; white-space: pre-wrap; overflow-wrap: anywhere; color: var(--d-fg); font-size: 13px; line-height: 20px }
.mac .pjc .ta::placeholder { color: var(--d-muted) }
.mac .pjc .row { display: flex; align-items: center; gap: 6px }
.mac .pjc .pill { display: inline-flex; align-items: center; gap: 6px; height: 28px; padding: 0 10px; border-radius: 8px; border: 1px solid var(--d-border); background: transparent; color: var(--d-fg); font-size: 12.5px }
.mac .pjc .pill small { color: var(--d-muted) }
.mac .pjc .sp { flex: 1 }
.mac .pjc .note { font-size: 11.5px; color: var(--d-muted) }`;
  document.head.append(st);
})();
// The same picker as Start a coding agent, opened in place above the bar.
const pjPicker = () => `<div class="agc-pick" style="right:110px;bottom:56px">
    <div class="agc-rail">${[["claude", "Claude Code"], ["codex", "Codex"], ["gemini", "Gemini CLI"], ["pi", "pi"], ["flsh", "flsh"], ["opencode", "OpenCode"]].map(([a, n], i) => `<button aria-pressed="${!i}" title="${n}"><span class="cc-ag ${a}"></span></button>`).join("")}</div>
    <div class="agc-list"><div class="agc-find"><div class="agc-search">Search Claude Code models…</div></div>
      <button class="on">Default<small>account default</small></button><button>model-large</button><button>model-small</button></div>
    <div class="agc-think"><h5><span>Thinking</span><span>Claude Code</span></h5>${["Low", "Medium", "High", "Extra high", "Max"].map(t => `<button class="${t === "Medium" ? "on" : ""}">${t}${t === "Medium" ? " ✓" : ""}</button>`).join("")}</div></div>`;
document.getElementById("desk").addEventListener("click", e => {
  if (!e.target.closest("[data-artfold]")) return;
  S.d.artfold = !S.d.artfold; render();
});
document.getElementById("desk").addEventListener("keydown", e => {
  if (e.key !== "Enter" || e.shiftKey || !e.target.closest(".pjc .ta")) return;
  e.preventDefault(); e.target.closest(".pjc").querySelector("[data-dgo]")?.click();
});
document.getElementById("desk").addEventListener("click", e => {
  if (!e.target.closest("[data-pjpick]")) return;
  S.d.pjpick = !S.d.pjpick; render();
});
pjBar = (o = {}) => {
  const noco = /checked out/.test(o.ph || "");
  return `<div class="pjc"><div class="box"><textarea class="ta" rows="1" title="Enter starts, Shift+Enter adds a line" placeholder="${noco ? "Add acme/widget to a machine to start an agent" : "Describe a task for a coding agent in acme/widget"}"></textarea>
    <div class="row"><button class="pill" data-toast="${noco ? "Machines with a coding agent: picking one adds acme/widget there" : "Machines with a checkout and a coding agent, updated when machines reconnect"}">${noco ? "Add to a machine" : "build-box"}</button>
      <button class="pill" data-toast="The checkout, a new worktree, or an existing worktree">Current checkout<small>as it is</small></button>
      <span class="sp"></span>
      <button class="pill" data-pjpick aria-expanded="${!!S.d.pjpick}">Claude Code<small>Default · Medium</small></button>
      <button class="dbtn pri" data-dgo="dAgentSession" data-dact="dset:ags:working">Start<kbd>${DI.ret}</kbd></button></div></div>
    ${S.d.pjpick ? pjPicker() : ""}</div>`;
};
