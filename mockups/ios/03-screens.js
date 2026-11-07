// ---- Screens ----
const R = {
  drawer: () => {
    const sel = S.drawerMode === "select" || S.drawerMode === "bulk", search = S.drawerMode === "search", held = S.drawerMode === "menu", heldGroup = S.drawerMode === "groupMenu";
    const hits = [
      ["Train to Girona", "...the hotel near the <mark>station</mark> has rooms from 89 euros, and...", false],
      ["Morning briefing", "You: what time does the <mark>station</mark> open on Sunday?", false],
    ];
    const chats = [
      // Pinned rows come first, groups and chats alike.
      ["Tokyo trip", "Sam: @beep can you find us a ryokan…", false, "groupChat", true],
      ["Boiler and Saturday weather", "Today, 09:38", true],
      ["Morning briefing", "Today, 08:00", false],
      ["Flat viewing questions", "Yesterday, 21:14", false],
      ["Train to Girona", "3 Oct, 17:02", false],
      ["Remember: short answers", "28 Sep, 11:45", false],
    ];
    return chatHtml() + `<div class="scrim" data-go="chat"></div><aside class="drawer">
      <div style="padding:18px 16px 8px;display:flex;align-items:center;gap:8px">
        <div class="title" style="flex:1;font-size:26px">Your chats</div>
        <button class="ib chunky" style="width:40px;height:40px" data-go="groupNew" aria-label="New group">${I.group}</button>
      </div>
      <div style="padding:4px 16px 8px"><div class="field" style="display:flex;align-items:center;gap:8px;color:var(${search ? "--m-ink" : "--m-dim"})">${I.search}${search ? "station" : "Find a conversation"}</div></div>
      ${search ? `<div style="flex:1;overflow:auto;padding:0 8px;display:grid;align-content:start;gap:2px">
        <div class="label" style="padding:4px 12px">Titles</div>
        <button class="chatrow" data-go="chat"><b>Changi <mark style="background:var(--m-t2);border-radius:3px">station</mark> pickup</b><span>12 Aug 2025, 22:40</span></button>
        <div class="label" style="padding:10px 12px 4px">In messages</div>
        ${hits.map(([t, l]) => `<button class="chatrow hit" data-go="chat"><b>${t}</b><span>${l}</span></button>`).join("")}
      </div>` : `${sel ? `<div style="display:flex;align-items:center;padding:0 8px 4px 20px"><b style="flex:1;font-weight:600">2 selected</b>
        <button class="text-btn" data-toast="All selected">All</button>
        <button class="ib" style="color:var(--m-accent)" data-act="drawerBulk" aria-label="Delete selected">${I.trash}</button>
        <button class="ib" data-act="drawerDone" aria-label="Done selecting">${I.x}</button></div>` : ""}
      <div style="flex:1;overflow:auto;padding:0 8px;display:grid;align-content:start;gap:2px">
        ${chats.map(([t, l, cur, to, pinned], i) => `<button class="chatrow ${(cur && !sel) || (sel && i < 2) ? "cur" : ""}" ${sel ? 'data-toast="Toggled"' : `data-go="${to || "chat"}"`} style="${sel ? "flex-direction:row;align-items:center;gap:12px;" : ""}${!sel && ((held && i === 1) || (heldGroup && i === 0)) ? "background:var(--m-t1)" : ""}">${sel ? `<span style="display:grid;place-items:center;width:18px;height:18px;border-radius:50%;flex:none;border:2px solid ${i < 2 ? "var(--m-accent)" : "var(--m-muted)"};color:var(--m-accent)">${i < 2 ? I.check.replace(/width="16" height="16"/, 'width="10" height="10"') : ""}</span><span style="display:grid;gap:2px;flex:1;min-width:0;text-align:left">` : ""}<b style="display:flex;align-items:center;gap:6px"><span style="flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis">${t}</span>${pinned && !sel ? `<span style="display:grid;color:var(--m-brand)" aria-label="Pinned">${I.pushpin.replace(/width="16" height="16"/, 'width="14" height="14"')}</span>` : ""}${to ? `<span style="display:grid;color:var(--m-brand)" aria-label="Group">${I.group.replace(/width="\d+" height="\d+"/, 'width="16" height="16"')}</span>` : ""}</b><span>${l}</span>${sel ? "</span>" : ""}</button>`).join("")}
      </div>`}
      ${held ? `<div class="menu" role="menu" style="left:24px;top:250px">
        <button role="menuitem" data-act="drawerSelect">${I.check}Select</button>
        <button role="menuitem" data-toast="Rename">${I.edit}Rename</button>
        <button role="menuitem" data-toast="Pinned">${I.pushpin}Pin</button>
        <button role="menuitem" data-go="share">${I.share}Share</button>
        <button role="menuitem" style="color:var(--m-error)" data-toast="Delete this conversation?">${I.trash}Delete</button>
      </div>` : ""}
      ${heldGroup ? `<div class="menu" role="menu" style="left:24px;top:200px">
        <button role="menuitem" data-act="drawerSelect">${I.check}Select</button>
        <button role="menuitem" data-toast="Unpinned">${I.pushpin}Unpin</button>
        <button role="menuitem" data-go="groupInvite">${I.link.replace(/width="\d+" height="\d+"/, 'width="16" height="16"')}Invite</button>
        <button role="menuitem" style="color:var(--m-error)" data-toast="Leave Tokyo trip?">${I.exit}Leave</button>
      </div>` : ""}
      ${search ? "" : `<div class="hr" style="margin:8px 20px"></div>
      <div style="display:flex;align-items:center;padding:0 10px 14px">
        <button class="text-btn" style="color:var(--m-muted);display:flex;gap:8px;align-items:center" data-go="settings">${I.gear}Settings</button>
        <button class="text-btn" style="color:var(--m-muted);display:flex;gap:8px;align-items:center" data-more="1"><svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><circle cx="5" cy="12" r="2"/><circle cx="12" cy="12" r="2"/><circle cx="19" cy="12" r="2"/></svg>More</button>
        <div style="flex:1"></div><span class="dot ${S.connected ? "" : "off"}"></span><span class="muted" style="font-size:11px;margin:0 12px 0 6px">${S.connected ? "Connected" : "Offline"}</span>
      </div>`}
    </aside>` + (S.drawerMode === "more" ? `<div class="scrim" data-act="drawerDone" style="z-index:8"></div>
    <div class="sheet" role="dialog" aria-label="More">
      <div class="grab"></div>
      <div style="display:grid;gap:8px">
        <div style="display:flex;gap:8px">
          <button class="tile chunky" style="background:${TINTS[0]}" data-go="memories">${I.brain}Memories</button>
          <button class="tile chunky" style="background:${TINTS[1]}" data-go="routines">${I.clock}Routines</button>
          <button class="tile chunky" style="background:${TINTS[2]}" data-go="skills">${skillIc}Skills</button>
        </div>
        <div style="display:flex;gap:8px">
          <button class="tile chunky" style="background:${TINTS[1]}" data-go="places">${I.pin}Places</button>
          <button class="tile chunky" style="background:${TINTS[0]}" data-go="apps">${I.puzzle}Apps</button>
          <button class="tile chunky" style="background:${TINTS[2]}" data-go="researches">${PR_ICON}Research</button>
        </div>
      </div>
    </div>` : "") + (S.drawerMode === "bulk" ? `<div class="dialog-wrap"><div class="dialog"><h3>Delete 1 conversation and leave 1 group?</h3>
      <div class="muted" style="font-size:14px;line-height:20px">The conversations and their messages are deleted for good. You stop getting the groups' messages.</div>
      <div class="end"><button class="text-btn" data-act="drawerSelect">Cancel</button><button class="text-btn" style="color:var(--m-accent)" data-act="drawerDone" data-toast="Deleted">Delete</button></div></div></div>` : "");
  },
  share: () => {
    const was = S.drawerMode; S.drawerMode = "list";
    const under = R.drawer(); S.drawerMode = was;
    const st = S.share, url = "https://app.example.com/s/3f9c2a7e41d84b0c9e5a6d1f08b2c7e4";
    if (st === "stopped") return under + `<div class="toast" style="bottom:40px">Stopped sharing. The link no longer opens.</div>`;
    return under + `<div class="scrim" data-act="shareClose" style="z-index:8"></div>
    <div class="sheet" role="dialog" aria-label="Share conversation">
      <div class="grab"></div>
      <div class="row" style="align-items:center"><span class="logo" style="width:52px;height:52px;border-radius:var(--radius);background:var(--m-t1)">${I.link}</span>
        <div class="grow"><h3>Share this chat</h3><span class="muted" style="font-size:13px">Boiler and Saturday weather</span></div></div>
      <div class="field" style="display:flex;align-items:center;font-family:var(--mono);font-size:12px;color:var(--m-ink);white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${url}</div>
      <div style="display:flex;gap:10px">
        <button class="pill chunky" style="flex:1;display:flex;align-items:center;justify-content:center;gap:8px;font-weight:600" data-toast="Link copied">${I.copy}Copy</button>
        <button class="pill chunky coral" style="flex:1;display:flex;align-items:center;justify-content:center;gap:8px;font-weight:600" data-toast="Share menu">${I.share}Send</button>
      </div>
      <div class="muted" style="font-size:12px;line-height:18px">${st === "updated" ? "Updated to the chat as it is now. The link is the same. " : "A copy of the chat as it is now. Later messages are not in it. "}Only your messages and Beep's replies are shared, not tool results, thoughts or your location. Anyone with Beep can open the link.</div>
      <div class="end" style="justify-content:space-between"><button class="text-btn" style="color:var(--m-error);padding:0" data-act="shareStop">Stop sharing</button><button class="go" style="box-shadow:0 3px 0 var(--m-ledge)" data-act="shareClose">Done</button></div>
    </div>`;
  },
  shared: () => {
    const st = S.shared;
    const bar = `<div class="bar" style="padding-left:6px"><button class="ib" data-go="chat" aria-label="Back">${I.back}</button>
      <div style="flex:1;min-width:0;display:grid;gap:3px"><div class="title" style="font-size:20px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${st === "loaded" ? "Train to Girona" : "Shared chat"}</div>
      ${st === "loaded" ? `<div class="sub">${I.link.replace('width="20" height="20"', 'width="13" height="13"')}Shared 3 Oct, 17:20</div>` : ""}</div></div>`;
    if (st === "loading") return bar + `<div class="body" style="display:grid;place-items:center"><span class="spin" style="width:22px;height:22px"></span></div>`;
    if (st === "gone") return bar + `<div class="body"><div class="empty">Not found.<br>The owner stopped sharing this chat, or deleted it.</div></div>`;
    return bar + `<div class="body">
      <div class="user">Train times to Girona on Friday?</div>
      <div class="reply">The 17:25 gets in at 18:43, and the 18:10 at 19:35. Both leave from Sants.</div>
      <div class="user">Any hotel near the station?</div>
      <div class="reply">The hotel by the station has rooms from 89 euros on Friday night, and it is a four minute walk to the old town.</div>
    </div>
    <div style="flex:none;padding:10px 16px 14px;display:grid;gap:8px">
      <button class="primary" style="display:flex;align-items:center;justify-content:center;gap:8px" data-go="chat" data-toast="Added to your chats">${I.plus}Continue in my chats</button>
      <div class="hint" style="padding:0">Read only. Continuing makes your own copy.</div>
    </div>`;
  },
  routines: () => {
    const list = [
      ["Morning briefing", "Give me a brief morning update: current time, routines coming up today, and a motivational note.", "Every day at 08:00", "Next: tomorrow 08:00"],
      ["Inbox sweep", "Check Fastmail for anything that needs a reply today.", "Weekdays at 13:30", "Next: today 13:30"],
      ["Plants", "Remind me to water the plants.", "Mon, Thu at 19:00", "Next: Thu 19:00"],
      ["Weekly review", "Summarize what I did this week from my calendar and mail.", "Every Sunday at 18:00", "Next: Sun 18:00"],
    ];
    const add = `<button class="ib chunky coral" style="width:42px;height:42px" data-go="newRoutine" aria-label="New routine">${I.plus}</button>`;
    return back("Routines", add) + `<div class="body" style="gap:8px">
      ${S.routinesEmpty ? `<div class="empty">No routines yet.<br>Schedule tasks for Beep to run on a timer.</div>` :
      list.map(([l, p, s, n], i) => i === S.deleted ? "" : `<div class="swipe${S.swipe && i === 0 ? ` to-${S.swipe} armed` : ""}" data-swipe="${i}" data-label="${esc(l)}">
        <div class="under del">Delete${I.trash}</div>
        <div class="under run">${I.play}Run now</div>
        <div class="card row"${S.swipe && i === 0 ? ` style="transform:translateX(${S.swipe === "del" ? -42 : 42}%)"` : ""}>
        <div class="grow"><b style="font-weight:600;font-size:14px">${l}</b><span class="muted" style="font-size:12px;line-height:18px">${p}</span>
          <span style="font-size:12px;color:var(--m-muted);margin-top:2px">${s}</span>
          ${S.enabled[i] ? `<span style="font-size:12px;color:var(--m-brand);font-weight:500">${n}</span>` : `<span style="font-size:12px;color:var(--m-dim)">Paused</span>`}</div>
        <button class="switch" role="switch" aria-checked="${S.enabled[i]}" aria-label="Enabled" data-routine="${i}"></button>
      </div></div>`).join("") + `<div class="hint">Swipe right to run now, left to delete</div>`}
    </div>${S.snack ? `<div class="snack">Routine deleted<button data-act="undo">Undo</button></div>` : ""}`;
  },
  apps: () => {
    const nokey = S.apps === "nokey";
    const fmList = (S.fm || "two") === "one" ? "alex@example.com" : "alex@example.com, alex@work.example";
    const connected = nokey ? [] : [["Fastmail", FM_LOGO, "#fff", fmList], ["Google Calendar", "31", TINTS[1]]];
    const more = [["Gmail", "G", TINTS[0]], ["Notion", "N", TINTS[2]], ["Slack", "S", TINTS[2]]];
    const row = ([n, l, tint, desc], state) => `<div class="row" style="align-items:center;padding:6px 0">
      <div class="logo" style="background:${tint}">${l}</div><div class="grow"><b style="font-weight:500;font-size:15px">${n}</b>${desc ? `<span class="muted" style="font-size:12px;display:block;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${desc}</span>` : ""}</div>${state}</div>`;
    return back("Apps") + `<div class="body" style="gap:6px">
      <div class="muted" style="font-size:14px;line-height:20px">Connect an app once, then just ask. "What's on my calendar?" or "Add this to Notion" work from any chat.</div>
      ${nokey ? `<div style="color:#B4531F;font-size:13px">Connecting apps needs a Composio API key on the server.</div>` : ""}
      ${connected.length ? `<div class="label" style="margin-top:8px">Connected</div>` + connected.map(a => `<div data-go="appDetail" style="cursor:pointer">` + row(a, `<span style="display:flex;align-items:center;gap:4px;font-size:12px;font-weight:600">Connected ${I.chev}</span>`) + `</div>`).join("") : ""}
      <div class="label" style="margin-top:8px">${connected.length ? "More apps" : "Available"}</div>
      ${more.map(a => row(a, nokey ? `<span class="muted" style="font-size:12px">Unavailable</span>` : `<button class="pill" style="min-height:34px;padding:0 14px;font-size:13px;font-weight:600" data-toast="Opening ${a[0]} sign-in">Connect</button>`)).join("")}
      <div class="label" style="margin-top:8px">Your servers</div>
      ${S.mcp === "none" ? `<div class="muted" style="font-size:13px;line-height:18px">Add any remote MCP server by its address. Beep asks before every action it takes there.</div>`
        : MCP_SERVERS.map(([n, l, host, ok], i) => `<div data-go="mcpDetail" data-set="mcpDetail:${ok ? "ok" : "signin"}" style="cursor:pointer">` + row([n, l, TINTS[i + 1]], ok
          ? `<span style="display:flex;align-items:center;gap:4px;font-size:12px;font-weight:600">${ok} tools ${I.chev}</span>`
          : `<button class="pill" style="min-height:34px;padding:0 14px;font-size:13px;font-weight:600" data-toast="Opening ${n} sign-in">Sign in</button>`).replace(`<b style="font-weight:500;font-size:15px">${n}</b>`, `<b style="font-weight:500;font-size:15px">${n}</b><span class="muted" style="font-size:12px;display:block">${host}</span>`) + `</div>`).join("")}
      <button class="pill" style="align-self:flex-start;margin-top:4px;font-weight:600" data-go="mcpAdd" data-set="mcpAdd:form">+ Add MCP server</button>
    </div>`;
  },
  appDetail: () => {
    const levels = [
      ["ask", "Ask first", "Beep shows what it is about to do and waits for you."],
      ["allow", "Always allow", "Beep sends, moves and deletes without asking."],
      ["read", "Read only", "Beep can search and read, never change anything."],
    ];
    const acts = Object.keys(S.allowed);
    const tagFor = a => S.level === "read" ? `<span class="tag" style="color:var(--m-muted);border-color:var(--m-soft)">Off</span>`
      : S.level === "allow" || S.allowed[a] ? `<span class="tag" style="background:var(--m-t1)">Allowed</span>` : `<span class="tag" style="background:var(--m-t2)">Asks</span>`;
    return `<div class="bar" style="padding-left:6px"><button class="ib" data-go="apps" aria-label="Back">${I.back}</button><span class="logo" style="width:34px;height:34px;background:#fff">${FM_LOGO}</span><div class="title" style="flex:1">Fastmail</div></div>
    <div class="body" style="gap:10px">
      ${(() => {
        // Fastmail connects several accounts; the first is the agent's default.
        const fm = S.fm || "two", accts = fm === "one" ? ["alex@example.com"] : ["alex@example.com", "alex@work.example"];
        return `<div class="label">Accounts</div>
      <div class="card" style="padding:2px 12px">${accts.map((a, i) => `<div class="toolrow" style="${i ? "border-top:1px solid var(--m-soft)" : ""}"><span class="grow" style="gap:0"><span>${a}</span>${i === 0 && accts.length > 1 ? `<span class="muted" style="font-size:11px">Default</span>` : ""}</span><button class="text-btn" style="color:var(--m-error);min-height:32px;font-size:13px" ${accts.length > 1 && i === 0 ? 'data-set="fm:removeFirst"' : 'data-set="fm:remove"'}>Remove</button></div>`).join("")}</div>
      ${fm === "adding" ? `<div class="sub" style="font-size:13px"><span class="spin"></span>Waiting for the browser sign-in…</div>`
        : `<button class="pill" style="align-self:flex-start;font-weight:600" data-set="fm:adding" data-toast="Opening Fastmail sign-in">+ Add another account</button>`}
      <div class="muted" style="font-size:12px;line-height:17px;padding:0 4px">Beep uses the first account unless you name another, like "check my work inbox". To add a different account, sign out of Fastmail in the browser first.</div>
      ${fm === "remove" || fm === "removeFirst" ? `<div class="dialog-wrap"><div class="dialog"><h3>Remove ${fm === "removeFirst" ? "alex@example.com" : "alex@work.example"}?</h3>
        <div class="muted">Beep stops using this account. Your other Fastmail accounts stay connected.</div>
        <div class="end"><button class="text-btn" data-set="fm:two">Cancel</button><button class="go" style="background:var(--m-error);color:#fff" data-set="fm:one" data-toast="Account removed">Remove</button></div></div></div>` : ""}`;
      })()}
      <div class="label" style="margin-top:6px">When Beep acts in Fastmail</div>
      ${levels.map(([k, t, d]) => `<button class="radio" role="radio" aria-checked="${S.level === k}" data-level="${k}"><span class="o"></span><span class="grow"><b style="font-weight:600;font-size:14px">${t}</b><span class="muted" style="font-size:12px;line-height:17px">${d}</span></span></button>`).join("")}
      <div class="label" style="margin-top:8px">Actions</div>
      <div class="card" style="padding:4px 12px">
        ${acts.map((a, i) => `<div class="toolrow" style="${i ? "border-top:1px solid var(--m-soft)" : ""}"><span style="flex:1">${a}</span>${tagFor(a)}${S.level === "ask" ? `<button class="switch" role="switch" aria-checked="${S.allowed[a]}" aria-label="Always allow ${a}" data-allow="${a}"></button>` : ""}</div>`).join("")}
      </div>
      <div class="muted" style="font-size:12px;padding:0 4px">Searching and reading mail is always allowed.</div>
      <div style="flex:1"></div>
      <button class="text-btn" style="color:var(--m-error);align-self:center" data-go="apps" data-toast="Fastmail disconnected">Disconnect Fastmail${(S.fm || "two") === "one" ? "" : " (all accounts)"}</button>
    </div>`;
  },
  mcpAdd: () => {
    const st = S.mcpAdd;
    const filled = st !== "form";
    const field = (label, ph, val, mono) => `<label class="fl">${label}<div class="field" style="display:flex;align-items:center;${mono ? "font-family:var(--mono);font-size:13px;" : ""}color:var(${filled ? "--m-ink" : "--m-dim"})">${filled ? val : ph}</div></label>`;
    const foot = {
      form: `<div class="muted" style="font-size:12px;line-height:17px">Use the server's https address. If it needs an account, Beep opens its sign-in in the browser.</div>`,
      connecting: `<div class="sub" style="font-size:13px"><span class="spin"></span>Connecting to the server…</div>`,
      signin: `<div class="sub" style="font-size:13px"><span class="spin"></span>Finish signing in to Team wiki in the browser, then come back here.</div>`,
      error: `<div style="color:var(--m-error);font-size:13px;line-height:18px">Could not reach this server. Check the address, or try again later.</div>`,
    }[st];
    return R.apps() + `<div class="dialog-wrap"><div class="dialog">
      <h3>Add MCP server</h3>
      ${field("Name", "Team wiki", "Team wiki")}
      ${field("Server URL", "https://example.com/mcp", "https://wiki.example.com/mcp", true)}
      ${foot}
      <div class="end"><button class="text-btn" data-go="apps">Cancel</button><button class="go" ${st === "form" || st === "error" ? `data-set="mcpAdd:connecting" data-go="mcpAdd"` : "disabled style=\"opacity:.5\""}>${st === "error" ? "Try again" : "Add"}</button></div>
    </div></div>`;
  },
  mcpDetail: () => {
    const ok = S.mcpDetail !== "signin";
    const tools = S.mcpDetail === "many" ? MCP_MANY : MCP_FEW;
    const on = mcpOn(tools), count = tools.filter(t => on[t]).length;
    const row = (t, i) => {
      const large = MCP_TOO_LARGE.includes(t);
      return `<div class="toolrow" style="${i ? "border-top:1px solid var(--m-soft)" : ""}"><span class="grow" style="gap:0"><span${large ? ' style="color:var(--m-muted)"' : ""}>${t}</span>${large ? `<span class="muted" style="font-size:11px">Too large for Beep to use</span>` : ""}</span>`
        + `<button class="switch" role="switch" aria-checked="${!large && !!on[t]}" aria-label="Use ${t}" ${large ? 'disabled style="opacity:.4"' : `data-mtool="${t}"`}></button></div>`;
    };
    return `<div class="bar" style="padding-left:6px"><button class="ib" data-go="apps" aria-label="Back">${I.back}</button><span class="logo" style="width:34px;height:34px;background:${TINTS[1]}">W</span><div class="title" style="flex:1">Team wiki</div></div>
    <div class="body" style="gap:10px">
      <div class="field" style="display:flex;align-items:center;font-family:var(--mono);font-size:12px;color:var(--m-ink);min-height:38px">https://wiki.example.com/mcp</div>
      <div class="sub" style="font-size:13px">${ok ? `<span class="dot"></span>Connected, ${count === tools.length ? `${count} tools` : `${count} of ${tools.length} tools on`}` : `<span class="dot" style="background:var(--m-error)"></span>Sign-in expired`}</div>
      ${ok ? `<div style="display:flex;justify-content:space-between;align-items:baseline;margin-top:6px"><span class="label">Tools</span><span class="label" style="${count > MCP_LIMIT ? "color:var(--m-error)" : ""}">${count} of ${MCP_LIMIT} on</span></div>
      <div class="card" style="padding:4px 12px">${tools.map(row).join("")}</div>
      <div class="muted" style="font-size:12px;line-height:17px;padding:0 4px">Beep uses up to ${MCP_LIMIT} tools from a server, so turn on the ones you need. It asks before every tool on a server you add. Tap Always when it asks to stop asking for this server.</div>`
      : `<div class="muted" style="font-size:13px;line-height:18px">The server stopped accepting Beep's sign-in. Sign in again to use its tools.</div>
      <button class="go" style="align-self:flex-start" data-toast="Opening Team wiki sign-in">Sign in again</button>`}
      <div style="flex:1"></div>
      <button class="text-btn" style="color:var(--m-error);align-self:center" data-go="apps" data-toast="Team wiki removed">Remove server</button>
      ${S.mcpAsk ? `<div class="dialog-wrap"><div class="dialog"><h3>Turn on more than ${MCP_LIMIT} tools?</h3>
        <div class="muted">Beep works best with ${MCP_LIMIT} tools or fewer from one server. With more, replies get slower and Beep is more likely to pick the wrong tool.</div>
        <div class="end"><button class="text-btn" data-mtoolno="1">Cancel</button><button class="go" data-mtoolok="1">Turn on</button></div></div></div>` : ""}
    </div>`;
  },
};
function qrSvg(seed = 0x2F8A69) {
  const N = 25; let a = seed;
  const rnd = () => { a = a + 0x6D2B79F5 | 0; let t = Math.imul(a ^ a >>> 15, 1 | a); t = t + Math.imul(t ^ t >>> 7, 61 | t) ^ t; return ((t ^ t >>> 14) >>> 0) / 4294967296; };
  const finder = (x, y) => (x < 8 && y < 8) || (x > N - 9 && y < 8) || (x < 8 && y > N - 9);
  let d = "";
  for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) {
    if (finder(x, y)) continue;
    if (x >= 10 && x <= 14 && y >= 10 && y <= 14) continue;
    const on = (y === 6 || x === 6) ? (x + y) % 2 === 0 : rnd() < .5;
    if (on) d += `M${x} ${y}h1v1h-1z`;
  }
  [[0, 0], [N - 7, 0], [0, N - 7]].forEach(([x, y]) => { d += `M${x} ${y}h7v7h-7zM${x + 1} ${y + 1}v5h5v-5zM${x + 2} ${y + 2}h3v3h-3z`; });
  return `<svg viewBox="0 0 ${N} ${N}" shape-rendering="crispEdges" role="img" aria-label="Link code"><path fill="#0A0E0C" fill-rule="evenodd" d="${d}"/><rect x="10.3" y="10.3" width="4.4" height="4.4" rx="1.1" fill="var(--m-accent)"/><path d="M11 11.5h3v2h-2l-1 1z" fill="none" stroke="currentColor" stroke-width=".35"/></svg>`;
}
const DOC = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>';
