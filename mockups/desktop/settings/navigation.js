const SETV = (() => {
  const { chev, sec, dsetGo, dt, generalBody, skinsHtml, macBody, acctBody, CODING, codingAgents } = SETTINGS_ACCOUNTS;
  const { credsBody, insBody } = SETTINGS_CREDENTIALS;
  const { HV, harnessPage, providers } = SETTINGS_HARNESSES;
  const { skillsBody } = SETTINGS_SKILLS;
  const PAGES = [["General", "general"], ["Appearance", "appearance"], ["This computer", "mac"], ["Machines", "machines"], ["Agent accounts", "acct"], ["Credentials", "keys"], ["Instructions", "instr"], ["Skills", "skills"]];
  const AGENT_SUBS = CODING.map(([a]) => a);
  const agentOf = v => { const a = v.split("-")[1]; return v.startsWith("acct-") && AGENT_SUBS.includes(a) ? a : null; };
  const pageOf = v => v.startsWith("acct") ? "acct" : v.startsWith("keys") ? "keys" : v.startsWith("mach") ? "machines" : v.startsWith("skills") ? "skills" : PAGES.some(([, k]) => k === v) ? v : "general";
  const subOf = v => v === "mach-build" ? "build-box" : agentOf(v) ? HV[agentOf(v)].name : null;
  const kids = p => p === "machines" ? ["mac-box", "laptop", "build-box", "win-box"].map(n => [n, "mach-build"])
    : p === "acct" ? CODING.map(([a, n]) => [n, `acct-${a}`]) : [];
  const pageKey = k => k === "machines" ? "machines-list" : k;
  const nav = v => {
    const p = pageOf(v), sub = subOf(v);
    return PAGES.map(([l, k]) => `<button class="cc-si" ${dsetGo(pageKey(k))}${p === k && !sub ? ' aria-current="true"' : ""}><span class="tt">${l}</span>${k === "machines" ? '<span class="n">4</span>' : ""}</button>`
      + (p === k ? kids(k).map(([kl, kv]) => `<button class="cc-si sub" ${dsetGo(kv)}${sub === kl ? ' aria-current="true"' : ""}><span class="tt">${kl}</span></button>`).join("") : "")).join("");
  };
  const BACK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M15 6l-6 6 6 6"/></svg>';
  const crumbs = v => {
    const p = pageOf(v), sub = subOf(v), title = PAGES.find(([, k]) => k === p)[0];
    if (!sub) return `<b>${title}</b>`;
    return `<button class="dset-back" ${dsetGo(pageKey(p))} aria-label="Back to ${title}">${BACK}</button><button class="dset-crumb" ${dsetGo(pageKey(p))}>${title}</button>${chev}<b>${sub}</b>`;
  };
  const host = inner => `<section><div class="dset-host">${inner}</div></section>`;
  const page = (v, machPages) => {
    const p = pageOf(v);
    if (machPages[v]) return `<section style="max-width:none">${machPages[v]()}</section>`;
    if (v === "machines") return host(`<div class="dhead"><span class="tt"><b>No machines linked</b></span></div><div class="dmuted" style="display:block;margin-bottom:10px">To add one, use Add a machine: it gives you a one-time command to run there.</div><button class="dbtn" data-dact="dset:tk:other" data-dgo="dAddTicket">Link another desktop</button><button class="dbtn" data-dgo="dAddMachine">Add a machine</button>`);
    if (p === "appearance") return sec("Appearance", skinsHtml());
    if (p === "mac") return `<section>${macBody()}</section>`;
    if (agentOf(v)) return host(harnessPage(v));
    if (p === "acct") return sec("Agent accounts", acctBody(v) + codingAgents());
    if (p === "keys") return sec("Credentials", credsBody(v));
    if (p === "instr") return host(insBody());
    if (p === "skills") return sec("Skill libraries", skillsBody(v));
    return sec("General", generalBody());
  };
  const frame = (v, machPages) => `<div class="cc-set-scrim" data-dgo="dHome"><div class="cc-set" role="dialog" aria-modal="true" aria-label="Settings" data-dgo="dWinSettings">
      <nav class="dset-nav" aria-label="Settings pages"><b>Settings</b><div class="dset-pages">${nav(v || "general")}</div><small class="dset-ver">Beep for macOS 0.2.0</small></nav>
      <div class="dset-body"><div class="dset-top"><span class="dset-crumbs">${crumbs(v || "general")}</span><kbd>esc</kbd></div><div class="dset-page dform">${page(v || "general", machPages)}</div></div></div></div>`;
  // Settings in the bar: every section in one column (settingsSections with sheet=false).
  const barColumn = () => `<div class="dform dsetcol" style="padding-top:12px">${[
    sec("General", generalBody()),
    sec("Appearance", skinsHtml()),
    `<section>${macBody()}</section>`,
    sec("Machines", dt("Other machines", "2 linked: build-box, gpu-box", '<button class="dbtn" data-dgo="dMachines">Show…</button>')),
    sec("Agent accounts", dt("Coding agents on each machine", "Versions and updates, on or off, binary paths, launch arguments and env vars for Claude Code, Codex, pi and the rest", '<button class="dbtn" data-dgo="dProviders">Open…</button>') + acctBody("acct")),
    sec("Credentials", credsBody("keys")),
    sec("Instructions", dt("Your rules for every agent", "One text for Claude Code, Codex, Gemini CLI and pi, with its versions, written into each agent's file on your machines", '<button class="dbtn" data-dgo="dWinInstructions">Open…</button>')),
    sec("Skills", dt("Skill libraries", "Packs of skills from Git repositories, installed in projects or globally", '<button class="dbtn" data-dact="dset:set:skills" data-dgo="dWinSettings">Open…</button>')),
  ].join("")}</div>`;
  return { frame, barColumn, insBody, providers };
})();

// The bar's Settings.
D.dSettings = () => deskScene(dBar(`<div class="dhead"><span class="tt"><b>Settings</b><small>Beep for macOS 0.2.0</small></span></div>${SETV.barColumn()}` + dHints([["esc", "close"]])));

// Which states the sheet can show. The machine page reuses its own state keys.
{
  const set0 = { mup: "", ins: "editor" };
  const rows = [["General", "general"], ["Appearance", "appearance"],
    ["This computer, off", "mac", { mac: "off" }], ["This computer, connecting", "mac", { mac: "connecting" }], ["This computer, failed", "mac", { mac: "failed" }], ["This computer, allow both", "mac", { mac: "perm" }], ["This computer, ready", "mac", { mac: "ready" }],
    ["Machines, none", "machines"], ["Machines", "machines-list"], ["Machines > build-box", "mach-build"], ["Machines > build-box, Updating", "mach-build", { mup: "updating" }], ["Machines > build-box, Restarting", "mach-build", { mup: "restarting" }], ["Machines > build-box, waits for sessions", "mach-build", { mup: "waiting" }], ["Machines > build-box, update failed", "mach-build", { mup: "failed" }], ["Machines > build-box, Projects in", "mach-build", { mup: "search" }], ["Machines > build-box, Remove", "mach-build", { mup: "asking" }], ["Machines > build-box, Log", "mach-build", { mup: "log" }], ["Machines > build-box, Log reading", "mach-build", { mup: "log-reading" }], ["Machines > build-box, no log", "mach-build", { mup: "log-none" }], ["Machines > build-box, Log empty", "mach-build", { mup: "log-empty" }], ["Machines > build-box, Log on an old node", "mach-build", { mup: "log-old" }], ["Machines > build-box, Sign in starting", "mach-build", { mup: "signin-starting" }], ["Machines > build-box, Sign in with a device code", "mach-build", { mup: "signin-opencode" }], ["Machines > build-box, Sign in with a pasted code", "mach-build", { mup: "signin-claude" }], ["Machines > build-box, Sign in failed", "mach-build", { mup: "signin-failed" }], ["Machines > build-box, Signed in", "mach-build", { mup: "signin-done" }],
    ["Agent accounts", "acct"], ["Agent accounts, Claude not signed in", "acct-none"], ["Agent accounts, approve in browser", "acct-flow"], ["Agent accounts, pasting a token", "acct-paste"], ["Agent accounts, token unreadable", "acct-unread"], ["Agent accounts, Codex device code", "acct-code"], ["Agent accounts, Codex starting", "acct-start"], ["Agent accounts, Codex sign-in failed", "acct-signfail"], ["Agent accounts, Codex API key", "acct-apikey"], ["Agent accounts, typing a key", "acct-keyform"], ["Agent accounts, could not load", "acct-fail"],
    ["Agent accounts > Claude Code", "acct-claude"], ["Agent accounts > Claude Code, Beep's token", "acct-claude-token"], ["Agent accounts > Claude Code, not signed in", "acct-claude-out"], ["Agent accounts > Claude Code, turned off", "acct-claude-off"], ["Agent accounts > Claude Code, updating", "acct-claude-updating"], ["Agent accounts > Claude Code, update failed", "acct-claude-fail"], ["Agent accounts > Claude Code, edited", "acct-claude-dirty"], ["Agent accounts > Claude Code, saved to all", "acct-claude-saved"], ["Agent accounts > Claude Code, restart node", "acct-claude-stale"], ["Agent accounts > Codex, installing its adapter", "acct-codex-adapting"], ["Agent accounts > Codex, adapter failed", "acct-codex-adapter"], ["Agent accounts > Claude Code, no machine", "acct-claude-none"], ["Agent accounts > Claude Code, asking", "acct-claude-asking"],
    ["Agent accounts > Codex", "acct-codex"], ["Agent accounts > Gemini CLI, not installed", "acct-gemini"], ["Agent accounts > pi", "acct-pi"], ["Agent accounts > OpenCode, not signed in", "acct-opencode"], ["Agent accounts > OpenCode, not reported", "acct-opencode-gone"], ["Agent accounts > flsh", "acct-flsh"],
    ["Credentials", "keys"], ["Credentials, adding Forgejo", "keys-gh"], ["Credentials, Forgejo checks", "keys-fjstates"], ["Credentials, replacing Forgejo token", "keys-fjreplace"], ["Credentials, replacing GitHub token", "keys-replace"], ["Credentials, token unreadable", "keys-unread"], ["Credentials, gh signed in", "keys-cli-in"], ["Credentials, gh signed out", "keys-cli-out"], ["Credentials, GitHub code", "keys-cli-code"], ["Credentials, no gh", "keys-cli-none"], ["Credentials, gh has two accounts", "keys-cli-many"], ["Credentials, pasting a GitHub token", "keys-cli-paste"], ["Credentials, two GitHub accounts", "keys-gh2"], ["Credentials, adding a GitHub account", "keys-gh-add"], ["Credentials, adding one: code", "keys-gh-code"], ["Credentials, loading", "keys-load"], ["Credentials, could not load", "keys-fail"], ["Credentials, no API keys", "keys-api-none"], ["Credentials, adding an API key", "keys-api-add"], ["Credentials, replacing an API key", "keys-api-replace"],
    ["Instructions", "instr"], ["Instructions, History", "instr", { ins: "history" }],
    ["Skills", "skills"], ["Skills, none", "skills-none"], ["Skills, a library open", "skills-open"], ["Skills, library menu", "skills-menu"], ["Skills, adding a library", "skills-add"], ["Skills, removing a library", "skills-remove"], ["Skills, finding libraries", "skills-scanning"], ["Skills, libraries found", "skills-found"]];
  // This one jumps to a Settings page.
  VARIANTS.dAgentAccounts = [["Signed in", "acct"], ["Claude not signed in", "acct-none"], ["Codex device code", "acct-code"]].map(([l, k]) => [l, () => { S.d.acc = k; render(); }, () => (S.d.acc || "acct") === k]);
  VARIANTS.dWinSettings = rows.map(([l, k, x]) => [l, () => { Object.assign(S.d, set0, { set: k }, x); render(); }, () => (S.d.set || "general") === k && Object.entries(x || {}).every(([a, b]) => S.d[a] === b)]);
  VARIANTS.dWinInstructions = [["Editor", "editor"], ["Editor, not saved", "editor-dirty"], ["Editor, nothing saved", "editor-first"], ["Loading", "loading"], ["History", "history"], ["History, a version shown", "history-open"], ["History, loading", "history-load"], ["History, none", "history-none"]]
    .map(([l, k]) => [l, () => { S.d.ins = k; render(); }, () => (S.d.ins || "editor") === k]);
  VARIANTS.dProviders = [["Codex", "codex"], ["Hermes", "hermes"], ["Claude Code", "claude"], ["Claude not signed in", "signin"], ["Beep's token", "token"], ["Gemini not installed", "missing"], ["pi turned off", "off"], ["OpenCode", "opencode"], ["flsh", "flsh"], ["Updating", "update"], ["Update failed", "fail"], ["No machine online", "none"]]
    .map(([l, k]) => [l, () => { S.d.pv = k; render(); }, () => (S.d.pv || "codex") === k]);
}
