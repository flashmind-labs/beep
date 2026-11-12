
// Cost and plan usage at the top right of every command center page. Claude reports its 5-hour
// and weekly limits during sessions; Codex's come from its own app-server on a machine. Dollars
// are list-price estimates; Codex reports tokens only.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .us { position: relative; margin-left: auto; flex: none }
.mac .cc-top .tools + .us, .mac .cc-top .us { margin-left: 10px }
.mac .us-pill { display: inline-flex; align-items: center; gap: 10px; height: 28px; padding: 0 10px; border-radius: 8px; border: 1px solid var(--d-border); background: transparent; color: var(--d-muted); font-size: 12px; font-variant-numeric: tabular-nums; white-space: nowrap }
.mac .us-pill b { color: var(--d-fg); font-weight: 500 }
.mac .us-plan { display: inline-flex; align-items: center; gap: 5px }
.mac .us-mk { display: inline-grid; place-items: center; width: 14px; height: 14px; color: var(--d-fg) }
.mac .us-mk svg { width: 14px; height: 14px }
.mac .us-pill b.warn { color: #F2C94C }
.mac .us-pop h5 .us-mk { margin-right: 6px; vertical-align: -2px }
.mac .us-pop { position: absolute; right: 0; top: calc(100% + 6px); z-index: 9; width: 360px; border: 1px solid var(--d-border); border-radius: 12px; background: #0f1011; box-shadow: 0 18px 50px rgba(0,0,0,.5); padding: 12px 14px; display: grid; gap: 12px; font-size: 12.5px; color: var(--d-fg) }
.mac .us-pop h5 { margin: 0 0 6px; font-size: 11.5px; color: var(--d-muted); font-weight: 600; display: flex; justify-content: space-between }
.mac .us-lim { display: grid; grid-template-columns: 90px 1fr auto; gap: 8px; align-items: center; padding: 3px 0 }
.mac .us-lim .bar { height: 6px; border-radius: 3px; background: var(--d-border); overflow: hidden }
.mac .us-lim .bar i { display: block; height: 100%; background: #4CB782 }
.mac .us-lim .bar i.warn { background: #F2C94C }
.mac .us-lim small, .mac .us-more { all: unset; display: block; padding: 4px 0; font-size: 12px; color: var(--d-dim); cursor: pointer } .mac .us-more:hover { color: var(--d-fg) }
.mac .us-row small { color: var(--d-muted) }
.mac .us-row { display: flex; justify-content: space-between; padding: 3px 0; font-variant-numeric: tabular-nums }
.mac .us-note { font-size: 11px; color: var(--d-muted); line-height: 15px }
.mac .us-seg { display: flex; gap: 2px; padding: 2px; margin-bottom: 8px; border: 1px solid var(--d-border); border-radius: 8px }
.mac .us-seg button { flex: 1; border: 0; background: transparent; color: var(--d-muted); font-size: 11.5px; padding: 3px 0; border-radius: 6px }
.mac .us-seg button.on { background: var(--d-border); color: var(--d-fg) }
.mac .us-pop h5.us-sub { margin-top: 8px }`;
  document.head.append(st);
})();
const usPill = () => {
  const open = !!S.d.uspop;
  const period = S.d.usperiod || "today";
  const mk = agent => `<span class="us-mk">${(window.__MARKS || {})[agent] || ""}</span>`;
  // Each plan's binding window: the one most used.
  const lim = (agent, used, warn) => `<span class="us-plan">${mk(agent)}<b class="${warn ? "warn" : ""}">${used}%</b></span>`;
  const win = (name, used, reset, warn) => `<div class="us-lim"><span>${name}</span><span class="bar"><i class="${warn ? "warn" : ""}" style="width:${used}%"></i></span><small>${used}% used · resets ${reset}</small></div>`;
  const data = {
    today: { label: "Today", total: "", tokens: "39.3M",
      projects: [["acme/widget", "", "38M"], ["acme/relay", "", "1.3M"]],
      harnesses: [["Claude Code, subscription", "~$61 · 38M"], ["Codex, subscription", "~$3.40 · 1.3M"]], models: [["claude-sonnet", "38M"], ["gpt-codex", "1.3M"]] },
    week: { label: "This week", total: "$7.95", tokens: "217.6M",
      projects: [["notes", "$5.85", "0.6M"], ["acme/widget", "$2.10", "150M"], ["acme/relay", "", "67M"]],
      harnesses: [["pi", "$5.85 · 0.6M"], ["flsh", "$2.10 · 0.8M"], ["Claude Code, subscription", "~$340 · 210M"], ["Codex, subscription", "~$16 · 6.2M"]], models: [["claude-sonnet", "210M"], ["gpt-codex", "6.2M"], ["other-model", "1.4M"]] },
    month: { label: "This month", total: "$26.03", tokens: "1.1B",
      projects: [["acme/widget", "$13.02", "800M"], ["notes", "$13.01", "1.5M"], ["acme/relay", "", "300M"], ["acme/site", "$1.20", "22M"], ["acme/infra", "$0.40", "9M"], ["acme/docs", "", "4M"], ["acme/cli", "", "1.1M"]],
      harnesses: [["flsh", "$13.01 · 1.5M"], ["pi", "$13.02 · 1.9M"], ["Claude Code, subscription", "~$1,780 · 1.1B"], ["Codex, subscription", "~$54 · 21M"]], models: [["claude-sonnet", "1.1B"], ["gpt-codex", "21M"], ["other-model", "3.4M"]] },
  };
  const d = data[period];
  const seg = Object.entries(data).map(([k, v]) => `<button data-usperiod="${k}" class="${k === period ? "on" : ""}">${v.label}</button>`).join("");
  const pop = open ? `<div class="us-pop">
      <div><h5><span>${mk("claude")}Claude Code · Max plan</span><span>as of 4 min ago</span></h5>
        ${win("5 hours", 38, "14:30")}
        ${win("This week", 76, "Mon", true)}</div>
      <div><h5><span>${mk("codex")}Codex · ChatGPT Pro</span><span>as of 12 min ago</span></h5>
        ${win("5 hours", 9, "15:10")}
        ${win("This week", 20, "Thu")}</div>
      <div><h5><span>${mk("flsh")}flsh · OpenRouter credits</span><span>as of just now</span></h5>
        <div class="us-row"><span>Account</span><span>$622.32 remaining</span></div>
        <div class="us-row"><small></small><small>$377.68 used · $1,000.00 total</small></div></div>
      <div><div class="us-seg">${seg}</div>
        <h5><span>By project</span></h5>
        ${(S.d.usmore ? d.projects : d.projects.slice(0, 5)).map(([n, c, t]) => `<div class="us-row"><span>${n}</span><span class="us-r">${c}${t ? `<small>${c ? " · " : ""}${t}</small>` : ""}</span></div>`).join("")}
        ${d.projects.length > 5 ? `<button class="us-more" data-usmore>${S.d.usmore ? "Show less" : `Show ${d.projects.length - 5} more`}</button>` : ""}
        <h5 class="us-sub"><span>By harness</span></h5>
        ${d.harnesses.map(([n, l]) => `<div class="us-row"><small>${n}</small><span class="us-r"><small>${l}</small></span></div>`).join("")}
        <h5 class="us-sub"><span>By model</span></h5>
        ${d.models.map(([n, t]) => `<div class="us-row"><small>${n}</small><span class="us-r"><small>${t}</small></span></div>`).join("")}
        <div class="us-row"><b>Total</b><b class="us-r">${d.total}<small>${d.total ? " · " : ""}${d.tokens}</small></b></div></div>
      ${/\$/.test(JSON.stringify(d)) ? `<div class="us-note">Dollars are API spend. ~ is what plan use would cost on the API, not in the total.</div>` : ""}
    </div>` : "";
  return `<span class="us"><button class="us-pill" data-uspop aria-expanded="${open}" title="Plan usage and cost" aria-label="Plan usage: Claude Code 76%, Codex 20%">${lim("claude", 76, true)}${lim("codex", 20)}<span>OpenRouter <b>$622</b> left</span></button>${pop}</span>`;
};
document.getElementById("desk").addEventListener("click", e => {
  if (!e.target.closest("[data-usmore]")) return;
  e.stopPropagation();
  S.d.usmore = !S.d.usmore; render();
}, true);
document.getElementById("desk").addEventListener("click", e => {
  const p = e.target.closest("[data-usperiod]");
  if (!p) return;
  e.stopPropagation();
  S.d.usperiod = p.dataset.usperiod; render();
}, true);
document.getElementById("desk").addEventListener("click", e => {
  if (!e.target.closest("[data-uspop]")) return;
  S.d.uspop = !S.d.uspop; render();
});
// The current app only shows plan usage on Home.
const usInject = html => !/<h2>Home<\/h2>/.test(html) ? html : html.replace(/(<div class="cc-top">[\s\S]*?)(<\/div>)/, (m, a, b) => a + usPill() + b);
{
  const baseWin = ccWin;
  ccWin = (side, main) => usInject(baseWin(side, main));
}
