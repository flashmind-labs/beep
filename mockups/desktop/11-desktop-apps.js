// ---- Desktop: apps ----
// Opened from Settings, Apps. Connect opens the app's sign-in in the browser; the row waits until it comes back.
const D_APPS = [
  ["Gmail", "G", "#F28B7D", "Search, read and send email"],
  ["Google Calendar", "31", "#8DB4E8", "See your events and add new ones"],
  ["Notion", "N", "#E3E4E6", "Find pages and add notes"],
  ["Slack", "S", "#D9A3C9", "Read channels and post messages"],
  ["Fastmail", FM_LOGO, "#fff", "Search, read and send email"],
];
const dLogo = (l, c, size = 28) => `<span class="dlg" style="background:${c};width:${size}px;height:${size}px">${l}</span>`;
D.dApps = () => {
  const st = S.d.apps || "list", wait = S.d.appWait || "Notion";
  const row = ([n, l, c, desc], end, attrs = "") => {
    const inner = `${dLogo(l, c)}<span class="tt"><b>${n}</b><span>${desc}</span></span><span class="end">${end}</span>`;
    return attrs ? `<button class="dappr" ${attrs}>${inner}</button>` : `<div class="dappr">${inner}</div>`;
  };
  const connected = D_APPS.slice(0, 2).map(a => row(a, `<span class="dtag own">Connected</span>${DI.chevR}`, `data-dgo="dAppPerm" aria-label="${a[0]} permissions"`)).join("");
  const more = D_APPS.slice(2).map(a => {
    if (st === "nokey") return row(a, `<span>Unavailable</span>`);
    if (st === "waiting" && a[0] === wait) return row(a, `<span class="dspin"></span><span>Waiting for browser…</span><button class="dbtn quiet" data-dact="appCancel">Cancel</button>`);
    return row(a, `<button class="dbtn pri" data-dact="appConnect:${a[0]}" data-toast="Opening ${a[0]} sign-in in the browser">Connect</button>`);
  }).join("");
  return deskScene(dBar(`<div class="dhead"><span class="tt"><b>Apps</b><small>Connect once, then just ask from any chat</small></span></div>
    <div class="dform" style="padding-top:12px;gap:8px">
      <div class="dsec" style="padding:0">Connected</div>
      <div class="dapps">${connected}</div>
      <div class="dsec" style="padding:6px 0 0">More apps</div>
      ${st === "nokey" ? `<div class="dmuted">Connecting more apps needs a Composio API key on the server.</div>` : ""}
      <div class="dapps">${more}</div>
      <div class="dsec" style="padding:6px 0 0">Your MCP servers</div>
      <div class="dapps">${MCP_SERVERS.map(([n, l, host, tools], i) => row([n, l, ["#A9A3E8", "#D9C27A"][i], host], st === "mcpWait" && !tools
        ? `<span class="dspin"></span><span>Waiting for browser…</span><button class="dbtn quiet" data-dact="appCancel">Cancel</button>`
        : tools ? `<button class="dbtn quiet" data-dgo="dMcp" aria-label="${n} tools"><span class="dtag own">${tools} tools</span>${DI.chevR}</button>`
        : `<span class="dtag ask">Sign-in expired</span><button class="dbtn pri" data-toast="Opening ${n} sign-in in the browser">Sign in</button>`)).join("")}</div>
      ${st === "mcpAdd" || st === "mcpBad" ? `<div class="dfield on"><span>Name</span><div class="f">Team notes</div></div>
      <div class="dfield${st === "mcpBad" ? " err" : ""}"><span>${st === "mcpBad" ? "Use an https address on the public internet" : "Server URL"}</span><div class="f" style="font-family:var(--mono);font-size:12.5px">${st === "mcpBad" ? "http://example.com/mcp" : `<span class="ph">https://example.com/mcp</span>`}</div></div>
      <div style="display:flex;gap:8px"><button class="dbtn pri" data-toast="Connecting to Team notes">Add server<kbd>${DI.ret}</kbd></button><button class="dbtn quiet" data-dact="mcpCancel">Cancel</button></div>`
      : `<div><button class="dbtn" data-dact="mcpAdd">Add MCP server</button></div>`}
      <div class="dmuted">Beep asks before every tool on a server you add.</div>
    </div>` + dHints([["esc", "back"]])));
};
// Opened from a server's tools in Apps. Mirrors the phone's MCP server screen.
D.dMcp = () => {
  const tools = S.d.mcpMany ? MCP_MANY : MCP_FEW;
  if (!S.d.mcpOn) { S.d.mcpOn = {}; tools.filter(t => !MCP_TOO_LARGE.includes(t)).slice(0, MCP_LIMIT).forEach(t => S.d.mcpOn[t] = true); }
  const on = S.d.mcpOn, count = tools.filter(t => on[t]).length;
  const rows = tools.map(t => {
    const large = MCP_TOO_LARGE.includes(t);
    if (S.d.mcpAsk === t) return `<div class="dactr"><span>${t}</span><span class="dmuted">More than ${MCP_LIMIT} tools makes replies slower and tool picks worse</span><button class="dbtn quiet" data-dact="mcpNo">Cancel</button><button class="dbtn pri" data-dact="mcpYes">Turn on</button></div>`;
    return `<div class="dactr"><span${large ? ' style="color:var(--d-muted)"' : ""}>${t}${large ? ` <small style="color:var(--d-muted)">Too large for Beep to use</small>` : ""}</span>`
      + `<button class="dswitch" role="switch" aria-checked="${!large && !!on[t]}" aria-label="Use ${t}" ${large ? 'disabled style="opacity:.4"' : `data-dact="mcpTool:${t}"`}></button></div>`;
  }).join("");
  return deskScene(dBar(`<div class="dhead">${dLogo("W", "#A9A3E8")}<span class="tt"><b>Team wiki</b><small style="font-family:var(--mono)">https://wiki.example.com/mcp</small></span></div>
    <div class="dform" style="padding-top:12px;gap:8px">
      <div class="dsec" style="padding:0;display:flex;justify-content:space-between"><span>Tools</span><span style="${count > MCP_LIMIT ? "color:var(--d-err)" : ""}">${count} of ${MCP_LIMIT} on</span></div>
      <div class="ddevs dfade" style="max-height:260px;overflow:auto">${rows}</div>
      <div class="dmuted">Beep uses up to ${MCP_LIMIT} tools from a server, so turn on the ones you need. It asks before every tool on a server you add.</div>
      <div><button class="dbtn warn" data-dgo="dApps" data-toast="Team wiki removed">Remove server</button></div>
    </div>` + dHints([["esc", "back"]])));
};
// Opened from a connected row in Apps. Mirrors the phone's app screen.
D.dAppPerm = () => {
  const lvl = S.d.appLevel || "ask";
  const allowed = S.d.appAllowed || (S.d.appAllowed = { "Send email": false, "Reply to email": true, "Move to trash": false, "Save a draft": true });
  const levels = [
    ["ask", "Ask first", "Beep shows what it is about to do and waits for you"],
    ["allow", "Always allow", "Beep sends, changes and deletes without asking"],
    ["read", "Read only", "Beep can search and read, never change anything"],
  ];
  const tag = a => lvl === "read" ? `<span class="dtag">Off</span>` : lvl === "allow" || allowed[a] ? `<span class="dtag own">Allowed</span>` : `<span class="dtag ask">Asks</span>`;
  // Fastmail connects several accounts; the first is the agent's default.
  const fm = S.d.fm || "", [an, al, ac] = fm ? ["Fastmail", FM_LOGO, "#fff"] : ["Gmail", "G", "#F28B7D"];
  const accts = fm === "one" ? ["alex@example.com"] : ["alex@example.com", "alex@work.example"];
  // The account list did not load: the rest of the page still shows, with Retry.
  const accounts = fm === "failed" ? `<div class="dsec" style="padding:0">Accounts</div>
      <div class="ddevs"><div class="dactr"><span style="color:var(--d-err)">Fastmail accounts did not load: the server took too long</span></div></div>
      <div><button class="dbtn" data-dact="fm:two">Retry</button></div>
      <div class="dsec" style="padding:6px 0 0">When Beep acts in Fastmail</div>`
    : !fm ? "" : `<div class="dsec" style="padding:0">Accounts</div>
      <div class="ddevs">${accts.map((a, i) => `<div class="dactr"><span>${a}${i === 0 && accts.length > 1 ? ` <small style="color:var(--d-muted)">Default</small>` : ""}</span>${fm === "remove" && i === 1
        ? `<span class="dmuted">Beep stops using this account</span><button class="dbtn quiet" data-dact="fm:two">Cancel</button><button class="dbtn warn" data-dact="fm:one" data-toast="alex@work.example removed">Remove</button>`
        : accts.length === 1 ? `<button class="dbtn quiet warn" data-dgo="dApps" data-toast="Fastmail disconnected">Remove</button>` : `<button class="dbtn quiet warn" data-dact="fm:remove">Remove</button>`}</div>`).join("")}</div>
      <div style="display:flex;gap:8px;align-items:center">${fm === "adding" ? `<span class="dspin"></span><span class="dmuted">Waiting for browser…</span><button class="dbtn quiet" data-dact="fm:two">Cancel</button>`
        : `<button class="dbtn" data-dact="fm:adding" data-toast="Opening Fastmail sign-in in the browser">Add another account</button>`}</div>
      <div class="dmuted">Beep uses the first account unless you name another, like "check my work inbox". To add a different account, sign out of Fastmail in the browser first.</div>
      <div class="dsec" style="padding:6px 0 0">When Beep acts in Fastmail</div>`;
  const acts = Object.keys(allowed).map(a => `<div class="dactr"><span>${a}</span>${tag(a)}${lvl === "ask" ? `<button class="dswitch" role="switch" aria-checked="${allowed[a]}" aria-label="Always allow ${a}" data-dact="appAllow:${a}"></button>` : ""}</div>`).join("");
  return deskScene(dBar(`<div class="dhead">${dLogo(al, ac)}<span class="tt"><b>${an}</b><small>${fm && accts.length > 1 ? `${accts.length} accounts` : "Connected as alex@example.com"}</small></span></div>
    <div class="dform" style="padding-top:12px;gap:8px">
      ${accounts || `<div class="dsec" style="padding:0">When Beep acts in Gmail</div>`}
      <div class="dradios" role="radiogroup">${levels.map(([k, t, d]) => `<button class="dradio" role="radio" aria-checked="${lvl === k}" data-dact="appLevel:${k}"><span class="o"></span><span class="tt"><b>${t}</b><small>${d}</small></span></button>`).join("")}</div>
      <div class="dsec" style="padding:6px 0 0">Actions</div>
      <div class="ddevs">${acts}</div>
      <div class="dmuted">Searching and reading is always allowed.</div>
      <div><button class="dbtn warn" data-dgo="dApps" data-toast="${an} disconnected">Disconnect ${an}${fm && accts.length > 1 ? " (all accounts)" : ""}</button></div>
    </div>` + dHints([["esc", "back"]])));
};
DESK_SCREENS[3].items.splice(2, 0, ["dApps", "Apps", "AppsView"], ["dAppPerm", "App permissions", "AppView"]);
DESK_SCREENS[3].items.splice(4, 0, ["dMcp", "MCP server", "McpServerView"]); VARIANTS.dMcp = [
  ["Few tools", () => { S.d.mcpMany = false; S.d.mcpOn = null; S.d.mcpAsk = null; render(); }, () => !S.d.mcpMany],
  ["Many tools", () => { S.d.mcpMany = true; S.d.mcpOn = null; S.d.mcpAsk = null; render(); }, () => S.d.mcpMany && !S.d.mcpAsk],
  ["Over 25", () => { S.d.mcpMany = true; S.d.mcpOn = null; S.d.mcpAsk = "Search people"; render(); }, () => S.d.mcpMany && !!S.d.mcpAsk],
];
VARIANTS.dApps = [
  ["List", () => { S.d.apps = "list"; render(); }, () => (S.d.apps || "list") === "list"],
  ["Connecting", () => { S.d.apps = "waiting"; S.d.appWait = "Notion"; render(); }, () => S.d.apps === "waiting"],
  ["No API key", () => { S.d.apps = "nokey"; render(); }, () => S.d.apps === "nokey"],
  ["Adding a server", () => { S.d.apps = "mcpAdd"; render(); }, () => S.d.apps === "mcpAdd"],
  ["Server URL refused", () => { S.d.apps = "mcpBad"; render(); }, () => S.d.apps === "mcpBad"],
  ["Server sign-in", () => { S.d.apps = "mcpWait"; render(); }, () => S.d.apps === "mcpWait"],
];
VARIANTS.dAppPerm = [
  ["Ask first", () => { S.d.appLevel = "ask"; render(); }, () => (S.d.appLevel || "ask") === "ask"],
  ["Always allow", () => { S.d.appLevel = "allow"; render(); }, () => S.d.appLevel === "allow"],
  ["Read only", () => { S.d.appLevel = "read"; render(); }, () => S.d.appLevel === "read"],
  ["Fastmail, two accounts", () => { S.d.fm = "two"; render(); }, () => S.d.fm === "two"],
  ["Fastmail, one account", () => { S.d.fm = "one"; render(); }, () => S.d.fm === "one"],
  ["Adding an account", () => { S.d.fm = "adding"; render(); }, () => S.d.fm === "adding"],
  ["Removing an account", () => { S.d.fm = "remove"; render(); }, () => S.d.fm === "remove"],
  ["Accounts not loading", () => { S.d.fm = "failed"; render(); }, () => S.d.fm === "failed"],
  ["Gmail", () => { S.d.fm = ""; render(); }, () => !S.d.fm],
];
