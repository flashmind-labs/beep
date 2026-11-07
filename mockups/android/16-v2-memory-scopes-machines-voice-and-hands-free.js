// ---- v2: memory scopes, machines, voice and hands-free ----
I.server = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><rect x="3" y="4" width="18" height="7" rx="2"/><rect x="3" y="13" width="18" height="7" rx="2"/><path d="M7 7.5h.01M7 16.5h.01"/></svg>';
I.folder = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M3 6.5A1.5 1.5 0 014.5 5H9l2 2.5h8.5A1.5 1.5 0 0121 9v9.5a1.5 1.5 0 01-1.5 1.5h-15A1.5 1.5 0 013 18.5z"/></svg>';
I.account = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"><circle cx="12" cy="12" r="9"/><circle cx="12" cy="10" r="3"/><path d="M6.5 18.5a6.5 6.5 0 0111 0"/></svg>';
I.speaker = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9h4l5-4v14l-5-4H4z"/><path d="M17 9a4 4 0 010 6M19.5 6.5a8 8 0 010 11"/></svg>';
I.micS = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><rect x="9" y="3" width="6" height="12" rx="3"/><path d="M5 11a7 7 0 0014 0M12 18v3"/></svg>';
const small = (svg, n) => svg.replace(/width="\d+" height="\d+"/, `width="${n}" height="${n}"`);

// What a memory is about when it is not the user (MemoryScope): a machine, a project or an account.
// Labels as the apps write them: the machine's name, "Project <name>", "<App> account".
const MSCOPES = {
  m: { label: "build-box", subject: "the machine build-box", icon: I.server },
  a: { label: "Github account", subject: "your Github account", icon: I.account },
  p: { label: "Project atlas", subject: "the project atlas", icon: I.folder },
};
const mScopeTag = k => `<span style="display:flex;align-items:center;gap:4px;font-size:11px;font-weight:500;color:var(--m-ink);margin:2px 0">${small(MSCOPES[k].icon, 12)}${MSCOPES[k].label}</span>`;
// Everything, You, then each subject by name; picking one says what it is about.
function mScopeChips(groups, attr, key) {
  const cur = (attr === "data-iset" ? S.i[key] : S[key]) || "all";
  const all = groups.flatMap(([, l]) => l), count = k => all.filter(x => k === "you" ? !x[3] : x[3] === k).length;
  const chip = (k, label, n) => `<button class="fchip" style="flex:none" aria-pressed="${cur === k}" ${attr}="${key}:${k}">${label}${n ? ` <span class="muted">${n}</span>` : ""}</button>`;
  const order = Object.keys(MSCOPES).sort((x, y) => MSCOPES[x].label.toLowerCase() < MSCOPES[y].label.toLowerCase() ? -1 : 1);
  const about = cur === "all" ? "" : `<div class="muted" style="font-size:12px;padding:4px 4px 0">What Beep knows about ${cur === "you" ? "you" : MSCOPES[cur].subject}.</div>`;
  return `<div class="chips" style="flex:none;flex-wrap:nowrap;overflow-x:auto;scrollbar-width:none;margin:0 -16px;padding:0 16px">${chip("all", "Everything")}${chip("you", "You", count("you"))}${order.map(k => chip(k, MSCOPES[k].label, count(k))).join("")}</div>${about}`;
}
VARIANTS.memories.splice(1, 0, ["About a machine", () => { S.memories = "list"; S.mscope = "m"; render(); }, () => S.memories === "list" && S.mscope === "m"]);
VARIANTS.memories[0] = ["List", () => { S.memories = "list"; S.mscope = "all"; render(); }, () => S.memories === "list" && (S.mscope || "all") === "all"];
