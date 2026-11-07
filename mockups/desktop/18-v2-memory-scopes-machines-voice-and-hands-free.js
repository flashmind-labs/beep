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
const dScopeTag = k => `<span class="dscope">${small(MSCOPES[k].icon, 11)}${MSCOPES[k].label}</span>`;
function dScopeChips() {
  const cur = S.d.mscope || "all";
  const all = MEMS.flatMap(([, l]) => l), count = k => all.filter(x => k === "you" ? !x[4] : x[4] === k).length;
  const chip = (k, label, n) => `<button class="dchip" aria-pressed="${cur === k}" data-dact="dset:mscope:${k}">${label}${n ? ` <span style="color:var(--d-muted)">${n}</span>` : ""}</button>`;
  return `<div class="dchips" style="padding:8px 16px 2px">${chip("all", "Everything")}${chip("you", "You", count("you"))}${["m", "a", "p"].map(k => chip(k, MSCOPES[k].label, count(k))).join("")}</div>`
    + (cur === "all" ? "" : `<div class="dmuted" style="padding:2px 18px 4px">${cur === "read" ? "What Beep learned or confirmed reading Gmail today at 9:02." : `What Beep knows about ${cur === "you" ? "you" : MSCOPES[cur].subject}.`}</div>`);
}
VARIANTS.dMemories.splice(1, 0, ["About a machine", () => { S.d.mem = "list"; S.d.mscope = "m"; render(); }, () => S.d.mem === "list" && S.d.mscope === "m"]);
VARIANTS.dMemories[0] = ["List", () => { S.d.mem = "list"; S.d.mscope = "all"; render(); }, () => S.d.mem === "list" && (S.d.mscope || "all") === "all"];
