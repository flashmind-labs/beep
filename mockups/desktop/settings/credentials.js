const SETTINGS_CREDENTIALS = (() => {
  const { sm, sec, skel, dsetGo, letter, box } = SETTINGS_ACCOUNTS;
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
  const choice = (login, active) => `<div style="display:flex;align-items:center;gap:8px;font-size:12.5px"><span style="color:var(--d-muted)">gh on this computer as @${login}</span>${active ? `<span style="font-size:10.5px;color:var(--d-dim)">active</span>` : ""}<span style="margin-left:auto"><button class="dbtn pri" ${sm} data-toast="Checked with GitHub and saved as @${login}">Use</button></span></div>`;
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
        in: { account: "gh on this computer as @you", acts: `<button class="dbtn pri" ${sm} data-toast="Checked with GitHub and saved as @you">Use gh sign-in</button>${paste(true, "keys-cli-paste")}` },
        out: { acts: `<button class="dbtn pri" ${sm} ${dsetGo("keys-cli-code")}>Sign in with GitHub</button>${paste(true, "keys-cli-paste")}`, help: "Opens GitHub in your browser, then saves the sign-in here" },
        code: { acts: `<button class="dbtn quiet" ${sm} ${dsetGo("keys-cli-out")}>Cancel</button>`, body: waitCode },
        none: { acts: paste(true, "keys-cli-paste"), help: `Make a token on ${lk(ghPage, "GitHub")}, then paste it.<br>Or install GitHub CLI (${cd("brew install gh")}) and sign in here` },
        many: { account: "gh on this computer has 2 accounts", acts: paste(true, "keys-cli-paste"), body: `<div style="display:grid;gap:6px">${choice("octo-one", true)}${choice("octo-two")}</div>` },
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
    hosts += fj("code.example.org", "@you", chip("checked from build-box", "This computer cannot reach code.example.org; build-box can") + cliChip("teacli", ["build-box"]));
    hosts += fj("forge.example.lan", "", chip("not reachable from your machines", "", "warn") + cliChip("teacli", ["mac-box"]));
    if (v === "keys-fjstates") {
      hosts += fj("git.old.example", "", chip("checking from your machines…") + cliChip("teacli", ["mac-box"]));
      hosts += fj("git.team.example", "", chip("not checked: no machine could reach it", "", "warn") + cliChip("teacli", MS));
      hosts += gitRow({ icon: FJ, name: "git.work.example", account: "@you", acts: editRm("git.work.example", "keys-fjreplace"), chips: chip("token refused", "401 Unauthorized", "err") + cliChip("teacli", MS), more: `<span class="derr">401 Unauthorized</span>` });
    }
    if (cli === "in") hosts += gitRow({ icon: FJ, name: "git.example.net", account: "teacli on this computer", usable: false, acts: `<button class="dbtn pri" ${sm} data-toast="Checked with git.example.net and saved">Use teacli sign-in</button>`, more: note("Signed in with teacli here, not in Beep yet") });
    const adding = v === "keys-gh-add" || v === "keys-gh-code"
      ? `<div style="padding:10px 0;border-bottom:1px solid var(--d-border);display:grid;gap:6px;font-size:13px"><div style="display:flex;align-items:center;gap:8px"><b style="font-weight:600">Add another GitHub account</b><span style="margin-left:auto"><button class="dbtn quiet" ${sm} ${dsetGo("keys-gh2")}>Cancel</button></span></div>
        <div style="display:grid;gap:6px;padding-left:15px">${v === "keys-gh-code" ? waitCode : `${choice("octo-three", true)}${choice("octo-four")}<div style="display:flex;gap:6px"><button class="dbtn" ${sm} ${dsetGo("keys-gh-code")}>Sign in with GitHub</button>${paste(true, "keys-cli-paste")}</div>${note("Sign in adds the account to gh here too. @octo-one is in Beep already.")}`}</div></div>` : "";
    if (adding) canAdd = false;
    const addingFj = v === "keys-gh";
    const fjNew = addingFj ? credForm({ title: "New Forgejo/Gitea server", url: "one token per server, teacli on every machine", urlField: true, html: `Make one on your server at ${cd("/user/settings/applications")}, with repository and issue access. Or run ${cd("teacli login https://<server> --token <token>")} on this computer and use that sign-in.` }) : "";
    const adds = (canAdd ? add("keys-gh-add", "+ Add another GitHub account") : "") + (addingFj ? "" : add("keys-gh", "+ Add a Forgejo/Gitea server"));
    return `<div style="display:grid">${head("Git hosts", 4)}${github}${hosts}${adding}${fjNew}${adds ? `<div style="display:flex;flex-wrap:wrap;gap:6px;margin-top:8px">${adds}</div>` : ""}</div>`;
  };

  // Instructions: instructions.ts.
  const insTab = (k, label, cur) => `<button class="dbtn tm-tab"${cur === k ? ' aria-current="true"' : ""} data-dact="dset:ins:${k}">${label}</button>`;
  const insSec = (title, small, body) => `<div><div class="cc-sec"><h4>${title}</h4>${small ? `<small>${small}</small>` : ""}</div>${body}</div>`;
  const insBody = () => {
    const v = S.d.ins || "editor";
    const cur = v.startsWith("history") ? "history" : "editor";
    const tabs = `<div class="in-tabs">${insTab("editor", "Editor", cur)}${insTab("history", "History", cur)}</div>`;
    const empty = (b, s) => `<div class="cc-panel cc-empty"><b>${b}</b>${s ? `<span>${s}</span>` : ""}</div>`;
    let body;
    if (v === "loading") body = empty("Loading your instructions…");
    else if (cur === "editor") {
      const dirty = v === "editor-dirty", first = v === "editor-first";
      const layers = insSec("How agents see it", "instruction sources", `<div class="cc-panel"><ol class="in-layers">
        <li><b>The agent's own rules</b><small>Built into Claude Code, Codex, Gemini CLI or pi</small></li>
        <li><b>Your text in its file</b><small>Your local CLAUDE.md or AGENTS.md, kept as is</small></li>
        <li class="me"><b>Your instructions</b><small>${first ? "Nothing saved yet, so agents get only the other layers" : "Version 7, supplied when Beep starts or resumes a session"}</small></li>
        <li><b>Beep's rules</b><small>Session setup and tool instructions supplied by Beep</small></li>
        <li><b>Project</b><small>The repository's CLAUDE.md or AGENTS.md</small></li>
        <li><b>Task</b><small>The prompt Beep starts the agent with</small></li></ol></div>`);
      body = `<div class="in-cols">${insSec("For all agents", first ? "Markdown · 0.0 of 16 KB" : "Markdown · 0.6 of 16 KB", `<div class="cc-panel in-ed">
        <div class="in-ver"><span><b>${first ? "Not saved yet" : "Version 7"}</b>${first ? "" : " · 2 days ago"}</span><span class="sp">${dirty ? "Not saved" : first ? "" : "Saved"}</span></div>
        <textarea aria-label="Your instructions" spellcheck="false" placeholder="# How I work&#10;&#10;- Write plainly.&#10;- Ask before deploying anything.">${first ? "" : IN_RULES}${dirty ? "\n- One more rule." : ""}</textarea>
        <div class="in-acts"><button class="dbtn pri"${dirty ? ' data-toast="Saved as version 8"' : " disabled"}>Save</button>${dirty ? '<button class="dbtn" data-dact="dset:ins:editor">Discard</button>' : ""}<button class="dbtn" data-dact="dset:ins:history">History</button></div></div>`)}
        <div class="in-sidecol">${layers}</div></div>`;
    } else if (v === "history-none") body = empty("No versions yet", "Each save makes one.");
    else if (v === "history-load") body = empty("Loading the versions…");
    else {
      const open = v === "history-open" ? 6 : 0;
      const h = (n, what, when, newest) => `<div class="in-hist"><div class="h"><b>${what}</b><small>${when}</small><span class="acts"><button class="dbtn small" data-dact="dset:ins:${open === n ? "history" : "history-open"}">${open === n ? "Hide" : "Show"}</button>${newest ? "" : `<button class="dbtn small" data-toast="Restored as version 8">Restore</button>`}</span></div>${open === n ? `<pre class="in-pre">${IN_RULES.split("\n").slice(0, 6).join("\n")}</pre>` : ""}</div>`;
      body = insSec("History", "newest first", `<div class="cc-panel">${h(7, "Version 7", "2 days ago", true)}${h(6, "Version 6 · Imported from Claude Code on build-box", "5 days ago")}${h(5, "Version 5 · Restored version 3", "1 week ago")}${h(4, "Version 4", "2 weeks ago")}</div>`);
    }
    return `<div class="cc-view in-view">${tabs}${body}</div>`;
  };

  // A coding agent's page: providers.ts, with its machine picker and summary in Settings.
  return { GH, FJ, ED, RM, chip, ico, editRm, cliChip, gitRow, note, lk, cd, MS, credForm, ghPage, waitCode, paste, ghAccount, choice, credsBody, insTab, insSec, insBody };
})();
