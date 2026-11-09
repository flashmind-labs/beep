const localSettings = () => S.d.noServer || S.d.homeAccount !== "subscribed";
const SETV = (() => {
  const { chev, tile, sm, dt, swi, sec, skel, dsetGo, generalBody, SK, SKL, skinsHtml, CHOICES, modelsBody, letter, waiting, appsList, appPerm, mcpPage, macBody, box, mrow, agentForm, claudeBox, codexBox, acctBody, CODING, codingAgents } = SETTINGS_ACCOUNTS;
  const { GH, FJ, ED, RM, chip, ico, editRm, cliChip, gitRow, note, lk, cd, MS, credForm, ghPage, waitCode, paste, ghAccount, choice, credsBody, insTab, insSec, insBody, accountBody } = SETTINGS_CREDENTIALS;
  const { HV, hvSwitch, nameLine, statusSmall, hvPicker, hvIn, hvField, hvEnv, hvRuntime, hvModelChoices, hvModelId, hvDefaultModel, hvModels, flshOwn, hvStatusBlock, failLine, harnessPage, providers } = SETTINGS_HARNESSES;
  const PAGES = [["General", "general"], ["Appearance", "appearance"], ["Models", "models"], ["Apps", "apps"], ["This computer", "mac"], ["Machines", "machines"], ["Agent accounts", "acct"], ["Credentials", "keys"], ["Instructions", "instr"], ["Beep server", "account"]];
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
    const shown = localSettings() ? PAGES.filter(([, k]) => ["general", "appearance", "mac", "machines", "acct", "keys", "instr"].includes(k)).map(([l, k]) => [k === "acct" ? "Coding agents" : l, k]) : v === "account-none" ? PAGES.filter(([, k]) => ["general", "appearance", "account"].includes(k))
      : v === "account-out" ? PAGES.filter(([, k]) => ["general", "appearance", "machines", "keys", "instr", "account"].includes(k)) : PAGES;
    return shown.map(([l, k]) => `<button class="cc-si" ${dsetGo(pageKey(k))}${p === k && !sub ? ' aria-current="true"' : ""}><span class="tt">${l}</span>${v !== "account-none" && k === "apps" ? '<span class="n">2</span>' : v !== "account-none" && k === "machines" ? '<span class="n">4</span>' : ""}</button>`
      + (p === k ? kids(k).map(([kl, kv]) => `<button class="cc-si sub" ${dsetGo(kv)}${sub === kl ? ' aria-current="true"' : ""}><span class="tt">${kl}</span></button>`).join("") : "")).join("");
  };
  const BACK = '<svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M15 6l-6 6 6 6"/></svg>';
  const crumbs = v => {
    const p = pageOf(v), sub = subOf(v), title = localSettings() && p === "acct" ? "Coding agents" : PAGES.find(([, k]) => k === p)?.[0] || "General";
    if (!sub) return `<b>${title}</b>`;
    return `<button class="dset-back" ${dsetGo(pageKey(p))} aria-label="Back to ${title}">${BACK}</button><button class="dset-crumb" ${dsetGo(pageKey(p))}>${title}</button>${chev}<b>${sub}</b>`;
  };
  const host = inner => `<section><div class="dset-host">${inner}</div></section>`;
  const page = (v, machPages) => {
    const p = pageOf(v);
    if (machPages[v]) return `<section style="max-width:none">${machPages[v]()}</section>`;
    if (v === "machines") return host(`<div class="dhead"><span class="tt"><b>No machines linked</b></span></div><div class="dmuted" style="display:block;margin-bottom:10px">To add one, use Add a machine: it gives you a one-time command to run there.</div><button class="dbtn" data-dact="dset:tk:other" data-dgo="dAddTicket">Link another desktop</button><button class="dbtn" data-dgo="dAddMachine">Add a machine</button>`);
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
    if (p === "acct") return sec(localSettings() ? "Coding agents" : "Agent accounts", (localSettings() ? "" : acctBody(v)) + codingAgents());
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
  return { frame, barColumn, appsList, appPerm, mcpPage, insBody, providers, generalBody };})();


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
  const set0 = { apps: "list", fm: "", mup: "", ins: "editor", setAuto: "on", noServer: false, homeAccount: "subscribed" };
  const rows = [["General", "general"], ["Appearance", "appearance"], ["Models, Auto on", "models"], ["Models, Auto off", "models", { setAuto: "off" }], ["Models, loading", "loading"], ["Models, could not load", "models-fail"],
    ["Apps", "apps"], ["Apps, connecting", "apps", { apps: "waiting" }], ["Apps, adding a server", "apps", { apps: "mcpAdd" }], ["Apps > Gmail", "app-gmail"], ["Apps > Fastmail", "app-fastmail", { fm: "two" }], ["Apps > Team wiki", "mcp-wiki"],
    ["This computer, off", "mac", { mac: "off" }], ["This computer, connecting", "mac", { mac: "connecting" }], ["This computer, failed", "mac", { mac: "failed" }], ["This computer, allow both", "mac", { mac: "perm" }], ["This computer, ready", "mac", { mac: "ready" }],
    ["Machines, none", "machines"], ["Machines", "machines-list"], ["Machines > build-box", "mach-build"], ["Machines > build-box, Updating", "mach-build", { mup: "updating" }], ["Machines > build-box, Restarting", "mach-build", { mup: "restarting" }], ["Machines > build-box, waits for sessions", "mach-build", { mup: "waiting" }], ["Machines > build-box, update failed", "mach-build", { mup: "failed" }], ["Machines > build-box, Projects in", "mach-build", { mup: "search" }], ["Machines > build-box, Remove", "mach-build", { mup: "asking" }], ["Machines > build-box, Log", "mach-build", { mup: "log" }], ["Machines > build-box, Log reading", "mach-build", { mup: "log-reading" }], ["Machines > build-box, no log", "mach-build", { mup: "log-none" }], ["Machines > build-box, Log empty", "mach-build", { mup: "log-empty" }], ["Machines > build-box, Log on an old node", "mach-build", { mup: "log-old" }], ["Machines > build-box, Sign in starting", "mach-build", { mup: "signin-starting" }], ["Machines > build-box, Sign in with a device code", "mach-build", { mup: "signin-opencode" }], ["Machines > build-box, Sign in with a pasted code", "mach-build", { mup: "signin-claude" }], ["Machines > build-box, Sign in failed", "mach-build", { mup: "signin-failed" }], ["Machines > build-box, Signed in", "mach-build", { mup: "signin-done" }],
    ["Coding agents, no server", "acct", { noServer: true }], ["Agent accounts", "acct"], ["Agent accounts, Claude not signed in", "acct-none"], ["Agent accounts, approve in browser", "acct-flow"], ["Agent accounts, pasting a token", "acct-paste"], ["Agent accounts, token unreadable", "acct-unread"], ["Agent accounts, Codex device code", "acct-code"], ["Agent accounts, Codex starting", "acct-start"], ["Agent accounts, Codex sign-in failed", "acct-signfail"], ["Agent accounts, Codex API key", "acct-apikey"], ["Agent accounts, typing a key", "acct-keyform"], ["Agent accounts, could not load", "acct-fail"], ["Agent accounts, server too old", "acct-old"],
    ["Agent accounts > Claude Code", "acct-claude"], ["Agent accounts > Claude Code, Beep's token", "acct-claude-token"], ["Agent accounts > Claude Code, not signed in", "acct-claude-out"], ["Agent accounts > Claude Code, turned off", "acct-claude-off"], ["Agent accounts > Claude Code, updating", "acct-claude-updating"], ["Agent accounts > Claude Code, update failed", "acct-claude-fail"], ["Agent accounts > Claude Code, edited", "acct-claude-dirty"], ["Agent accounts > Claude Code, saved to all", "acct-claude-saved"], ["Agent accounts > Claude Code, restart node", "acct-claude-stale"], ["Agent accounts > Codex, installing its adapter", "acct-codex-adapting"], ["Agent accounts > Codex, adapter failed", "acct-codex-adapter"], ["Agent accounts > Claude Code, no machine", "acct-claude-none"], ["Agent accounts > Claude Code, asking", "acct-claude-asking"], ["Agent accounts > Claude Code, server too old", "acct-claude-old"],
    ["Agent accounts > Codex", "acct-codex"], ["Agent accounts > Gemini CLI, not installed", "acct-gemini"], ["Agent accounts > pi", "acct-pi"], ["Agent accounts > OpenCode, not signed in", "acct-opencode"], ["Agent accounts > OpenCode, not reported", "acct-opencode-gone"], ["Agent accounts > flsh", "acct-flsh"],
    ["Credentials", "keys"], ["Credentials, adding Forgejo", "keys-gh"], ["Credentials, Forgejo checks", "keys-fjstates"], ["Credentials, replacing Forgejo token", "keys-fjreplace"], ["Credentials, replacing GitHub token", "keys-replace"], ["Credentials, token unreadable", "keys-unread"], ["Credentials, gh signed in", "keys-cli-in"], ["Credentials, gh signed out", "keys-cli-out"], ["Credentials, GitHub code", "keys-cli-code"], ["Credentials, no gh", "keys-cli-none"], ["Credentials, gh has two accounts", "keys-cli-many"], ["Credentials, pasting a GitHub token", "keys-cli-paste"], ["Credentials, two GitHub accounts", "keys-gh2"], ["Credentials, adding a GitHub account", "keys-gh-add"], ["Credentials, adding one: code", "keys-gh-code"], ["Credentials, loading", "keys-load"], ["Credentials, could not load", "keys-fail"],
    ["Instructions", "instr"], ["Instructions, History", "instr", { ins: "history" }],
    ["Beep server", "account"], ["Beep server, signing out", "account-signout"], ["Beep server, signed out", "account-out"], ["Beep server, not reachable", "account-down"], ["Beep server, linked", "account-member"], ["Beep server, unlinking", "account-unlink"], ["Beep server, loading", "account-load"], ["Beep server, no account", "account-none"]];
  VARIANTS.dWinSettings = [["Current: mesh settings", () => { Object.assign(S.d, { homeAccount: "mesh", noServer: true, set: "general" }); render(); }, () => localSettings()], ["Server settings preview", () => { Object.assign(S.d, { homeAccount: "subscribed", noServer: false, set: "general" }); render(); }, () => !localSettings()], ...rows.map(([l, k, x]) => [l, () => { Object.assign(S.d, set0, { set: k }, x); render(); }, () => (S.d.set || "general") === k && Object.entries(x || {}).every(([a, b]) => S.d[a] === b)])];
  // These two jump to a Settings page.
  VARIANTS.dAgentAccounts = [["Signed in", "acct"], ["Claude not signed in", "acct-none"], ["Codex device code", "acct-code"]].map(([l, k]) => [l, () => { S.d.acc = k; render(); }, () => (S.d.acc || "acct") === k]);
  VARIANTS.dWinInstructions = [["Editor", "editor"], ["Editor, not saved", "editor-dirty"], ["Editor, nothing saved", "editor-first"], ["Loading", "loading"], ["History", "history"], ["History, a version shown", "history-open"], ["History, loading", "history-load"], ["History, none", "history-none"]]
    .map(([l, k]) => [l, () => { S.d.ins = k; render(); }, () => (S.d.ins || "editor") === k]);
  VARIANTS.dProviders = [["Codex", "codex"], ["Hermes", "hermes"], ["Claude Code", "claude"], ["Claude not signed in", "signin"], ["Beep's token", "token"], ["Gemini not installed", "missing"], ["pi turned off", "off"], ["OpenCode", "opencode"], ["flsh", "flsh"], ["Updating", "update"], ["Update failed", "fail"], ["No machine online", "none"]]
    .map(([l, k]) => [l, () => { S.d.pv = k; render(); }, () => (S.d.pv || "codex") === k]);
}
