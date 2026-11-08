// ---- Screens ----
const R = {
  permissions: () => {
    const card = (k, icon, title, desc, tint) => `<div class="card row" style="align-items:center">
      <div class="logo" style="background:${tint}">${icon}</div>
      <div class="grow"><b style="font-weight:600">${title}</b><span class="muted" style="font-size:13px;line-height:19px">${desc}</span></div>
      ${S.perms[k] ? `<span style="color:var(--m-ok)">${I.check}</span>` : `<button class="pill" style="min-height:36px;padding:0 12px" data-perm="${k}">Allow</button>`}
    </div>`;
    return `<div class="body" style="padding:28px 20px;gap:16px">
      <div class="chunky" style="width:56px;height:56px;border-radius:var(--radius);background:#0B0E1D;color:#fff;display:grid;place-items:center;font-family:var(--display);font-weight:800;font-size:24px">${BI(28)}</div>
      <div class="big">Almost ready.</div>
      <div class="muted" style="line-height:22px">A couple of permissions to make Beep more useful. You can skip any of these.</div>
      ${card("loc", I.pin, "Location", "Let Beep answer questions about nearby places, weather, and directions.", TINTS[0])}
      ${card("notif", I.bell, "Notifications", "Get notified when scheduled routines finish or when Beep has something for you.", TINTS[1])}
      <div class="card row" style="align-items:center">
        <div class="logo" style="background:${TINTS[2]}">${I.key}</div>
        <div class="grow"><b style="font-weight:600">${S.keyImported ? "Signed in as sample-account" : "Moving from another phone?"}</b><span class="muted" style="font-size:13px;line-height:19px">${S.keyImported ? "From the key file. This replaces the new account on this phone." : "You are alex. Import the key file you saved on your old phone to keep that account."}</span></div>
        ${S.keyImported ? `<span style="color:var(--m-ok)">${I.check}</span>` : `<button class="pill" style="min-height:36px;padding:0 12px" data-act="importKey">Import key</button>`}
      </div>
      ${S.computer === "joined" ? `<div class="card row" style="align-items:center">
        <div class="logo" style="background:${TINTS[0]}">${I.laptop}</div>
        <div class="grow"><b style="font-weight:600">On MacBook's account</b><span class="muted" style="font-size:13px;line-height:19px">This phone now shows MacBook's chats and memories.</span></div>
        <span style="color:var(--m-ok)">${I.check}</span>
      </div>` : `<div class="card" style="display:grid;gap:10px">
        <div class="row" style="align-items:center">
          <div class="logo" style="background:${TINTS[0]}">${I.laptop}</div>
          <div class="grow"><b style="font-weight:600">Using Beep on a computer?</b><span class="muted" style="font-size:13px;line-height:19px">Scan the code Beep shows on your computer to use the same chats and memories here.</span></div>
        </div>
        <div style="display:flex;gap:4px;justify-content:flex-end;align-items:center">
          <button class="text-btn" style="color:var(--m-muted);min-height:36px" data-set="onb:true" data-go="typeCode">Type code</button>
          <button class="pill" style="min-height:36px;padding:0 12px" data-set="onb:true" data-go="joinConfirm" data-toast="Camera opens to scan">Scan code</button>
        </div>
      </div>`}
      <div style="flex:1"></div>
      <button class="primary" data-go="empty">Continue to Beep</button>
      <button class="text-btn" style="color:var(--m-muted)" data-go="empty">Skip for now</button>
    </div>`;
  },
  empty: () => {
    const server = S.starters === "server" || S.starters === "limit" || S.starters === "loading";
    const spin = `<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12a9 9 0 1 1-2.6-6.4"/><path d="M21 4v5h-5"/></svg>`;
    const more = !server ? "" : S.starters === "limit"
      ? `<button class="text-btn" disabled style="align-self:center;display:flex;gap:6px;align-items:center;color:var(--m-muted);opacity:.6;font-size:13px;margin-top:4px">${spin}New ideas in 7 min</button>`
      : S.starters === "loading"
      ? `<button class="text-btn" disabled style="align-self:center;display:flex;gap:6px;align-items:center;color:var(--m-muted);font-size:13px;margin-top:4px"><span style="display:inline-flex;animation:spin .9s linear infinite">${spin}</span>Finding new ideas</button>`
      : `<button class="text-btn" data-act="newIdeas" style="align-self:center;display:flex;gap:6px;align-items:center;color:var(--m-muted);font-size:13px;margin-top:4px">${spin}New ideas</button>`;
    const items = server ? [
      ["Reply to Alex", "Draft a reply to Alex saying Thursday morning works for the plumber.", I.copy],
      ["Saturday plans", "What's the weather for Saturday and anything on my calendar?", I.clock],
      ["Inbox check", "Anything in Fastmail that needs a reply today?", I.search],
    ] : [
      ["Remember something", "Remember that I prefer short, practical answers.", I.brain],
      ["Search the web", "Search the web for ", I.search],
      ["Set a reminder", "Remind me to ", I.clock],
    ];
    const clip = S.starters === "clip" ? `<div class="banner" style="background:var(--m-t1)"><div class="grow"><b style="font-weight:600;font-size:13px">From your clipboard</b><span class="muted" style="font-size:12px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">https://example.com/flat-listing-3b</span></div><button class="pill" style="min-height:32px;padding:0 10px;font-size:12px" data-go="chat">Summarize</button></div>` : "";
    // What the triage agent kept for later: its own card, never folded into a reply.
    // Tap opens a chat about it; the cross dismisses. Three show, the rest fold behind "more".
    const ups = S.upd === "none" ? [] : UPDATES.slice(0, S.upd === "many" ? 5 : 2);
    const urow = ([l, app, t, w], i) => `<div style="display:flex;gap:10px;align-items:flex-start;padding:9px 12px${i ? ";border-top:1px solid var(--m-ledge)" : ""}">
        <span class="logo" style="width:26px;height:26px;font-size:11px;background:${TINTS[i % 3]};flex:none">${l}</span>
        <button class="grow" style="all:unset;flex:1;min-width:0;cursor:pointer;display:grid;gap:2px" data-go="chat" data-set="updChat:true"><span style="font-size:13px;line-height:18px">${t}</span><span class="muted" style="font-size:11.5px">${app} · ${w}</span></button>
        <button class="ib muted" style="width:28px;height:28px" aria-label="Dismiss" data-toast="Dismissed">${I.x}</button></div>`;
    const upd = ups.length ? `<div class="banner" style="flex-direction:column;align-items:stretch;gap:0;padding:4px 0;background:var(--m-t1)">
        <div style="display:flex;align-items:center;padding:6px 12px 4px"><b style="font-weight:600;font-size:13px;flex:1">Updates</b><button class="text-btn" style="font-size:12px;color:var(--m-muted)" data-toast="All dismissed">Clear</button></div>
        ${ups.slice(0, 3).map(urow).join("")}
        ${ups.length > 3 ? `<button class="text-btn" style="font-size:12px;color:var(--m-muted);padding:6px 12px;text-align:left;border-top:1px solid var(--m-ledge)" data-toast="Shows the other ${ups.length - 3}">${ups.length - 3} more</button>` : ""}
      </div>` : "";
    return topBar() + `<div class="body" style="padding-top:28px">
      ${clip}${upd}
      <div class="big" style="font-size:32px">Hey. What can I help with?</div>
      <div class="muted" style="line-height:23px;margin-bottom:16px">Find an answer. Untangle a thought.<br>Keep something worth remembering.</div>
      ${items.map(([t, p, ic], i) => { const held = S.held === i; return `<button class="chunky sugg${held ? " held" : ""}" style="animation-delay:${120 + i * 90}ms" data-hold="${i}" data-go="chat"><span class="ico" style="background:${TINTS[i % 3]}">${ic}</span><span style="min-width:0;flex:1"><b>${t}</b>${held ? `<span class="p">${esc(p)}</span>` : ""}</span></button>`; }).join("")}
      ${more}
    </div>` + composer();
  },
  chat: chatHtml,
  approve: () => {
    const st = S.approve, many = st === "many", waiting = st === "ask" || many;
    if (st === "notif") return `<div class="shade">
      <div class="notif">
        <div class="app"><i>${BI(11)}</i>Beep &middot; now</div>
        <div class="t">Allow Fastmail: send email?</div>
        <div class="b"><b>To:</b> alex@example.com
<b>Subject:</b> Re: Boiler
<b>Body:</b> Hi Alex, Thursday between 9 and 12 works for me. Thanks for sorting it out!</div>
        <div class="a"><button data-act="deny">Deny</button><button data-act="allow">Allow</button></div>
      </div>
      <div class="notif" style="opacity:.75;animation:none"><div class="app"><i style="background:#8A6FD1">M</i>Mail &middot; 12m</div><div class="t">Renfe</div><div class="b">Your ticket to Girona</div></div>
    </div>`;
    const status = { ask: '<span class="spin"></span>', many: '<span class="spin"></span>', allowed: `<span class="stamp" style="color:var(--m-ok);display:grid">${I.check}</span>`,
      denied: `<span class="stamp" style="color:var(--m-error);display:grid">${I.x}</span>`, skipped: `<span class="stamp" style="color:var(--m-dim);display:grid">${I.x}</span>` }[st];
    const mails = ["Delving Bitcoin summary", "Your weekly digest", "Order shipped", "Webinar reminder", "Price drop alert", "New sign-in"];
    const card = many ? `<div class="ask">
        <div class="head"><span class="logo" style="width:32px;height:32px;font-size:13px;background:var(--m-t0)">F</span>
          <div class="grow"><b style="font-weight:600;font-size:14px">Delete email?</b><span class="muted" style="font-size:12px">Fastmail asks before acting</span></div>${I.shield}</div>
        <div class="recs">${mails.map(m => `<div class="rec"><b>${m}</b><span>news@example.com</span></div>`).join("")}</div>
        <div class="recn">Swipe to see all ${mails.length}</div>
        <div class="acts">
          <button class="tick" role="checkbox" aria-checked="${S.always}" data-act="always"><i>${S.always ? I.check.replace('width="16" height="16"', 'width="12" height="12"') : ""}</i>Always allow delete email</button>
          <button class="pill" style="min-height:38px;padding:0 14px" data-act="deny">Deny</button>
          <button class="go" style="min-height:38px;box-shadow:0 3px 0 var(--m-ledge);white-space:nowrap" data-act="allow">Allow all ${mails.length}</button>
        </div></div>`
      : waiting ? `<div class="ask">
        <div class="head"><span class="logo" style="width:32px;height:32px;font-size:13px;background:var(--m-t0)">F</span>
          <div class="grow"><b style="font-weight:600;font-size:14px">Send this email?</b><span class="muted" style="font-size:12px">Fastmail asks before sending</span></div>${I.shield}</div>
        <div class="kv"><span>To</span><span>Alex Puig</span><span>Subject</span><span>Re: Boiler</span></div>
        <div class="draft">Hi Alex, Thursday between 9 and 12 works for me. Thanks for sorting it out!</div>
        <div class="acts">
          <button class="tick" role="checkbox" aria-checked="${S.always}" data-act="always"><i>${S.always ? I.check.replace('width="16" height="16"', 'width="12" height="12"') : ""}</i>Always allow sending email</button>
          <button class="pill" style="min-height:38px;padding:0 14px" data-act="deny">Deny</button>
          <button class="go" style="min-height:38px;box-shadow:0 3px 0 var(--m-ledge)" data-act="allow">Allow</button>
        </div></div>`
      : `<div class="muted" style="font-size:12px;display:flex;align-items:center;gap:6px;padding:0 4px">${I.shield}${{ allowed: S.always ? "Allowed, and always allowed from now on" : "Allowed once", denied: "Denied", skipped: "Not answered, you wrote again, so not sent", resolved: "No longer waiting" }[st]}</div>`;
    const reply = { ask: "", many: "", allowed: "Sent. I told Alex Thursday between 9 and 12 works.", denied: "Okay, I didn't send it. Want me to change the wording?", skipped: "Sure, I'll leave the email to Alex unsent for now." }[st];
    return topBar() + `<div class="body">
      <div class="user">${many ? "Move those newsletters to the trash" : "Tell Alex Thursday works"}</div>
      <div class="chip" style="background:var(--m-t0);align-self:flex-start"><span class="ini">F</span>Fastmail: ${many ? "delete email" : "send email"}<span class="st">${status}</span></div>
      ${card}
      ${reply ? `<div class="reply">${reply}</div>` : ""}
    </div>` + composer();
  },
  runNotif: () => {
    const st = S.run;
    const mail = `<div class="notif" style="opacity:.75;animation:none"><div class="app"><i style="background:#8A6FD1">M</i>Mail &middot; 12m</div><div class="t">Renfe</div><div class="b">Your ticket to Girona</div></div>`;
    const ongoing = st === "replying" || st === "two" ? `<div class="label" style="color:#5F6662;padding:4px 8px 0">Silent</div>
      <div class="notif" data-go="chat" style="cursor:pointer;padding-bottom:12px"><div class="app"><i>${BI(11)}</i>Beep</div>
        <div class="t" style="display:flex;align-items:center;gap:8px">Beep is replying<span class="spin"></span></div>
        <div class="b">${st === "two" ? "2 chats" : "Boiler and Saturday weather"}</div></div>` : "";
    const result = st === "done" ? `<div class="notif"><div class="app"><i>${BI(11)}</i>Beep &middot; now</div>
        <div class="t">Boiler and Saturday weather</div>
        <div class="b"><b>Beep:</b> Yes, Alex replied this morning: the plumber can come Thursday between 9 and 12, and she asked if that works for you.</div>
        <div class="a"><button data-go="chat">Reply</button></div></div>` : "";
    const connected = st === "nogoogle" ? `<div class="label" style="color:#5F6662;padding:4px 8px 0">Silent</div>
      <div class="notif" data-go="chat" style="cursor:pointer;opacity:.75;animation:none"><div class="app"><i>${BI(11)}</i>Beep</div>
        <div class="t">Beep is connected</div></div>` : "";
    return `<div class="shade">${result}${st === "done" || st === "idle" ? mail : mail + ongoing + connected}
      ${st === "idle" ? `<div class="hint" style="color:#5F6662">Nothing from Beep. The app is idle; notices arrive through Firebase.</div>` : ""}
      ${st === "nogoogle" ? `<div class="hint" style="color:#5F6662">No Google Play services, so no Firebase. Beep stays connected to receive notices.</div>` : ""}</div>`;
  },
  math: () => {
    const v = S.math;
    const quad = `<span class="tex"><i>x</i> = <span class="frac"><span>&minus;<i>b</i> &plusmn; &radic;<span class="sqrt"><i>b</i><sup>2</sup> &minus; 4<i>ac</i></span></span><span>2<i>a</i></span></span></span>`;
    const wide = `<span class="tex"><i>A</i> = <i>P</i><span style="font-size:1.6em;vertical-align:-.15em">(</span>1 + <span class="frac"><span><i>r</i></span><span><i>n</i></span></span><span style="font-size:1.6em;vertical-align:-.15em">)</span><sup><i>nt</i></sup> = 10000<span style="font-size:1.6em;vertical-align:-.15em">(</span>1 + <span class="frac"><span>0.035</span><span>12</span></span><span style="font-size:1.6em;vertical-align:-.15em">)</span><sup>12&middot;10</sup> &asymp; 14182.53</span>`;
    if (v === "wide") return topBar() + `<div class="body">
      <div class="user">What does $10,000 grow to at 3.5% a year, compounded monthly, over 10 years?</div>
      <div class="reply">Use the compound interest formula with <span class="tex"><i>r</i> = 0.035</span>, <span class="tex"><i>n</i> = 12</span> and <span class="tex"><i>t</i> = 10</span>:</div>
      <div class="reply mathblock">${wide}</div>
      <div class="reply">So about $14,183, of which $4,183 is interest.</div>
    </div>` + composer();
    if (v === "bad") return topBar() + `<div class="body">
      <div class="user">How do I solve 2x&sup2; + 3x &minus; 2 = 0?</div>
      <div class="reply">Use the quadratic formula:</div>
      <div class="reply"><span class="mathsrc block">x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}{2a}</span></div>
      <div class="reply">The closing brace after the square root is missing, so the source shows as written. Inline: <span class="mathsrc">$\\sqrt{b^2$</span> stays text too.</div>
    </div>` + composer();
    return topBar() + `<div class="body">
      <div class="user">How do I solve <span class="tex"><i>2x</i><sup>2</sup> + 3<i>x</i> &minus; 2 = 0</span>?</div>
      <div class="reply">Any equation of the form <span class="tex"><i>ax</i><sup>2</sup> + <i>bx</i> + <i>c</i> = 0</span> solves with the quadratic formula:</div>
      <div class="reply mathblock">${quad}</div>
      <div class="reply">Here <span class="tex"><i>a</i> = 2</span>, <span class="tex"><i>b</i> = 3</span> and <span class="tex"><i>c</i> = &minus;2</span>, so the discriminant is <span class="tex">9 + 16 = 25</span> and its root is 5:</div>
      <div class="reply mathblock"><span class="tex"><i>x</i> = <span class="frac"><span>&minus;3 &plusmn; 5</span><span>4</span></span>, &nbsp; so &nbsp;<i>x</i> = <span class="frac"><span>1</span><span>2</span></span> &nbsp;or&nbsp; <i>x</i> = &minus;2</span></div>
    </div>` + composer();
  },
  place: () => {
    const grab = S.place === "grab" || S.place === "grabMenu", local = S.place === "local" || S.place === "localMenu";
    const menu = grab || local, open = S.place === "grabMenu" || S.place === "localMenu", dir = S.place === "directions" || menu;
    const [ask, say, name, addr] = grab
      ? ["How do I get to Marina Bay Sands?", "It's about 10 minutes by car from you. The route is on the map card below.", "Marina Bay Sands", "10 Bayfront Ave, Singapore"]
      : local
      ? ["How do I get to the Prado?", "It's about 15 minutes by car from you. The route is on the map card below.", "Museo del Prado", "Calle de Ruiz de Alarcón 23, Madrid"]
      : dir
      ? ["How do I get to Dubai Mall?", "It's about 20 minutes by car from you. The route is on the map card below.", "Dubai Mall", "Financial Centre Rd, Downtown Dubai"]
      : ["Any good ramen near me?", "Two close by: Tomo is quieter, Kinoya has the better broth. Kinoya is on the map card below.", "Kinoya", "The Onyx Tower 2, The Greens"];
    const item = (app, note, toast) => `<button role="menuitem" data-toast="${toast}" style="display:grid;gap:0"><span>${app}</span><span class="muted" style="font-size:11px">${note}</span></button>`;
    const items = grab
      ? item("Grab", S.grabApp ? "In the Grab app" : "Through Uber: tap Grab there", S.grabApp ? "Opening Grab" : "Opening Uber. Tap Grab there") + item("Uber", "Uber's own rides", "Opening Uber to " + name)
      : item("Uber", "Drop-off set to " + name, "Opening Uber to " + name) + item("Cabify", "Set the drop-off there", "Opening Cabify")
        + item("FreeNow", "Set the drop-off there", "Opening FreeNow");
    const btn = (label, icon, primary, msg) => `<button class="cbtn chunky" style="${primary ? "background:var(--m-accent)" : ""}" data-toast="${msg}">${icon}${label}</button>`;
    return topBar() + `<div class="body">
      <div class="user">${ask}</div>
      <div class="chip" style="background:var(--m-t1);align-self:flex-start"><span class="ini">M</span>${dir ? "Maps: directions" : "Maps: search"}<span class="st" style="color:var(--m-ok)">${I.check}</span></div>
      <div class="reply">${say}</div>
      <div class="card" style="padding:0;overflow:hidden">
        <div class="map"><span class="pinm">${I.pin.replace('width="20" height="20"', 'width="16" height="16"')}</span><small>&copy; OpenStreetMap</small></div>
        <div style="padding:14px;display:grid;gap:3px">
          <div class="title" style="font-size:17px">${name}</div>
          <div class="muted" style="font-size:13px">${addr}</div>
          <div style="display:flex;flex-wrap:wrap;gap:10px;margin-top:9px">
            ${btn("Directions", I.dir, true, "Opening Google Maps navigation")}
            ${menu ? `<span style="position:relative"><button class="cbtn chunky" data-act="rideMenu" aria-haspopup="menu" aria-expanded="${open}">${I.car}Ride ${I.chev}</button>
              ${open ? `<div class="menu" role="menu" style="left:0;bottom:52px;min-width:220px">${items}</div>` : ""}</span>` : dir ? btn("Uber", I.car, false, "Opening Uber to " + name) : ""}
            ${btn("Open in Maps", I.map, false, "Opening Google Maps")}
          </div>
        </div>
      </div>
    </div>` + composer();
  },
  recall: () => {
    const st = S.recall, none = st === "none";
    const found = [
      ["Has a reservation at the Grand Harbour Hotel, Singapore.", "plan", "Oct 4 to Oct 9"],
      ["Flies back to Barcelona on Friday the 9th.", "plan", "until Oct 9"],
      ["Prefers taking a taxi to walking after dark.", "preference", ""],
    ];
    const chev = I.chev.replace('class="chev"', `class="chev" style="transform:rotate(${st === "open" ? 180 : 0}deg);color:var(--m-muted)"`);
    const list = st === "open" ? `<div style="margin:-4px 0 0 4px;border:1px solid var(--m-soft);border-radius:var(--radius);background:var(--m-surface);overflow:hidden">
        ${found.map(([c, cat, when], i) => `<div style="padding:9px 12px;${i ? "border-top:1px solid var(--m-soft)" : ""}"><div style="font-size:13px;line-height:18px">${c}</div><div class="muted" style="font-size:11px">${cat}${when ? ", " + when : ""}</div></div>`).join("")}
      </div>` : "";
    return topBar() + `<div class="body">
      <div class="user">Uber to my hotel</div>
      <button class="chip" style="background:var(--m-t1);align-self:flex-start"${none ? "" : ` data-act="recallToggle" aria-expanded="${st === "open"}"`}><span class="ini">${I.brain.replace('width="20" height="20"', 'width="15" height="15"')}</span>Recalled memories &middot; ${none ? "None found" : "3 found"}<span class="st" style="color:var(--m-ok)">${I.check}</span>${none ? "" : chev}</button>
      ${list}
      <div class="reply">${none ? "I don't have your hotel saved. Which one are you staying at?" : "Opening Uber to the Grand Harbour Hotel. Pick the ride and confirm it there."}</div>
    </div>` + composer();
  },
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
  images: () => {
    const st = S.images;
    // Made-up pictures: a sky, a block with planted ledges and the street, drawn so the mock needs no files.
    const pic = (w, sky, block, green, i) => `<svg viewBox="0 0 ${w} 200" preserveAspectRatio="xMidYMid slice" style="display:block;width:100%;height:100%">
      <rect width="${w}" height="200" fill="${sky}"/><rect x="${w * .18}" y="${40 - i * 8}" width="${w * .6}" height="${150 + i * 8}" fill="${block}"/>
      ${[0, 1, 2, 3, 4].map(k => `<rect x="${w * .18}" y="${60 + k * 26 - i * 4}" width="${w * .6}" height="7" fill="${green}"/>`).join("")}
      <rect y="178" width="${w}" height="22" fill="#6b7570"/></svg>`;
    const IMGS = [
      [300, "#cfe3ec", "#e8e4dc", "#5f9a6a", "Faceted podium on the street side"],
      [160, "#d9e6ea", "#d8d2c6", "#4f8a5c", "Green facade from the corner"],
      [260, "#c8dbe6", "#efeae1", "#6aa874", "Planted ledges, east flank"],
      [180, "#dbe7ee", "#e2ddd3", "#578f62", "Entrance on Handy Road"],
    ];
    const tile = ([w, sky, block, green, alt], i) => `<figure style="flex:none;margin:0;display:grid;gap:6px;width:${w}px;scroll-snap-align:start">
      <button data-act="viewer" style="all:unset;cursor:pointer;height:200px;border-radius:var(--radius);overflow:hidden;background:var(--m-code)">${pic(w, sky, block, green, i)}</button>
      <figcaption style="font-size:12px;line-height:16px;color:var(--m-muted)">${alt}</figcaption></figure>`;
    const shown = st === "fails" ? IMGS.filter((_, i) => i !== 1) : IMGS;
    const row = st === "loading"
      ? `<div style="display:flex;gap:10px;overflow:hidden;margin:0 -16px;padding:0 16px">${IMGS.map(([w]) => `<div style="flex:none;width:${w}px;height:200px;border-radius:var(--radius);background:var(--m-code)"></div>`).join("")}</div>`
      : st === "allfail"
      ? `<div style="display:grid;gap:4px">${IMGS.slice(0, 2).map(i => `<a style="color:var(--m-brand);text-decoration:underline" data-toast="Opens the page">${i[4]}</a>`).join("")}</div>`
      : st === "single"
      ? `<figure style="margin:0;display:grid;gap:6px"><div style="height:220px;border-radius:var(--radius);overflow:hidden">${pic(340, ...IMGS[0].slice(1, 4), 0)}</div><figcaption style="font-size:12px;color:var(--m-muted)">${IMGS[0][4]}</figcaption></figure>`
      : `<div style="display:flex;gap:10px;overflow-x:auto;scroll-snap-type:x mandatory;scroll-padding:0 16px;margin:0 -16px;padding:0 16px 4px;scrollbar-width:none">${shown.map(tile).join("")}</div>`;
    const viewer = S.viewer ? `<div style="position:absolute;inset:0;background:#000;z-index:5;display:flex;flex-direction:column">
      <div class="bar" style="padding-left:6px;color:#fff;background:none"><button class="ib" data-act="closeViewer" aria-label="Close" style="color:#fff">${I.x}</button>
        <div style="flex:1;font-size:14px;color:#ccc">1 of 4</div><button class="text-btn" style="color:#fff" data-toast="Opens the page">Open</button></div>
      <div style="flex:1;display:grid;place-items:center"><div style="width:100%;height:300px">${pic(390, ...IMGS[0].slice(1, 4), 0)}</div></div>
      <div style="padding:14px 20px 26px;color:#ddd;font-size:14px;text-align:center">${IMGS[0][4]}</div></div>` : "";
    return topBar() + `<div class="body" style="position:relative">
      <div class="user">I want to see the sides of the school</div>
      <div class="chip" style="background:var(--m-t1);align-self:flex-start"><span class="ini">W</span>Searched the web<span class="st" style="color:var(--m-ok)">${I.check}</span></div>
      <div class="reply">Here are the flanks. The podium side is faceted, and the green facade wraps the corner.</div>
      ${row}
      <div class="reply">The full set on the architect's page covers every side, including the street corners.</div>
    </div>${viewer}` + composer();
  },
  file: () => {
    const st = S.file;
    if (st === "image") {
      // An image Beep made in its file folder shows as a picture, not a file card.
      const chart = `<svg viewBox="0 0 320 200" style="display:block;width:100%;height:100%;background:#fff">
        <line x1="30" y1="170" x2="300" y2="170" stroke="var(--m-dim)"/><line x1="30" y1="20" x2="30" y2="170" stroke="var(--m-dim)"/>
        <path d="M30 120 C60 40 90 40 120 120 S180 200 210 120 S270 40 300 120" fill="none" stroke="var(--m-brand)" stroke-width="3"/>
        <path d="M30 120 C60 70 90 70 120 120 S180 170 210 120 S270 70 300 120" fill="none" stroke="var(--m-accent)" stroke-width="2" stroke-dasharray="6 5"/>
        <text x="40" y="34" font-size="13" fill="var(--m-ink)">2_interference.png</text></svg>`;
      const viewer = S.fileView ? `<div style="position:absolute;inset:0;background:#000;z-index:5;display:flex;flex-direction:column">
        <div class="bar" style="padding-left:6px;color:#fff;background:none"><button class="ib" data-act="closeFileView" aria-label="Close" style="color:#fff">${I.x}</button>
          <div style="flex:1;font-size:14px;color:#ccc">2 of 3</div><button class="text-btn" style="color:#fff" data-toast="Share menu">Share</button></div>
        <div style="flex:1;display:grid;place-items:center"><div style="width:100%;height:230px">${chart}</div></div>
        <div style="padding:14px 20px 26px;color:#ddd;font-size:14px;text-align:center">Waves that line up add, opposite ones cancel</div></div>` : "";
      return topBar() + `<div class="body" style="position:relative">
        <div class="user">Explain how a quantum computer breaks RSA, with drawings</div>
        <div class="chip" style="background:var(--m-t1);align-self:flex-start"><span class="ini">C</span>Ran code<span class="st" style="color:var(--m-ok)">${I.check}</span></div>
        <div class="reply">The second drawing is the key idea: interference.</div>
        <div style="display:grid;gap:4px"><div style="height:200px;border-radius:var(--radius);overflow:hidden;cursor:pointer" data-act="openFileView">${chart}</div>
          <div class="muted" style="font-size:13px">Waves that line up add, opposite ones cancel</div></div>
        <div class="reply">Shor's algorithm uses it to find the repeat that gives up the key.</div>
      </div>${viewer}` + composer();
    }
    const doc = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>';
    const right = st === "loading"
      ? `<span class="spin" style="width:18px;height:18px"></span>`
      : st === "gone" ? "" : `<button class="ib" data-toast="Share menu" aria-label="Share">${I.share}</button>`;
    return topBar() + `<div class="body">
      <div class="user">Send me the boarding pass PDF</div>
      <div class="chip" style="background:var(--m-t1);align-self:flex-start"><span class="ini">F</span>Fastmail: read email<span class="st" style="color:var(--m-ok)">${I.check}</span></div>
      <div class="reply">Here is your Singapore Airlines boarding pass for SQ012 tomorrow.</div>
      <div class="card row" style="align-items:center;gap:12px;padding:10px 6px 10px 12px;cursor:pointer${st === "gone" ? ";opacity:.6" : ""}" data-toast="${st === "gone" ? "No longer available" : "Opens in a PDF viewer"}">
        <div style="flex:none;width:40px;height:40px;border-radius:var(--radius);display:grid;place-items:center;background:var(--m-t0);color:var(--m-ink)">${doc}</div>
        <div style="flex:1;min-width:0;display:grid;gap:2px">
          <div style="font-weight:600;font-size:14px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">BoardingPass.pdf</div>
          <div class="muted" style="font-size:12px">${st === "gone" ? "No longer available" : st === "loading" ? "Downloading..." : "PDF, 26 KB"}</div>
        </div>${right}
      </div>
      <div class="reply">Gate closes 40 minutes before departure.</div>
    </div>` + composer();
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
  menu: () => {
    const m = S.menu, hl = "position:relative;background:var(--m-t1);border-radius:var(--radius);padding:6px 8px;margin:-6px -8px";
    const earlier = `<div class="user">Train times to Girona on Friday?</div>
    <div class="reply">The 17:25 gets in at 18:43, and the 18:10 at 19:35.</div>`;
    if (m === "deleted") return topBar() + `<div class="body">${earlier}</div>` + composer();
    const prompt = m === "prompt";
    return topBar() + `<div class="body">${earlier}
    <div class="user"${prompt ? ` style="outline:3px solid var(--m-t1);outline-offset:2px"` : ""}>What's a good way to ask for the deposit back?</div>
    <div class="chip" style="background:var(--m-t1);align-self:flex-start"><span class="ini">W</span>Searching the web<span class="st" style="color:var(--m-ok)">${I.check}</span></div>
    <div class="reply" style="${prompt ? "" : hl}">Keep it short and specific. Say when you moved out, that the flat was left in good condition, and ask for the full deposit by a date two weeks out. Attach the move-out photos.</div>
    <div class="menu" style="${prompt ? "right:16px;top:150px" : "left:40px;top:300px"}">
      <button data-toast="Copied">${I.copy}Copy</button>
      <button data-go="chat">${I.quote}Quote in reply</button>
      <button data-toast="Share sheet">${I.share}Share</button>
      ${prompt ? `<button data-act="deleteTurn" style="color:var(--m-error)">${I.trash}Delete</button>` : ""}
    </div>
  </div>` + composer();
  },
  // Each queued message sends as its own turn, in order, once the reply before it finishes.
  // X puts one back in the composer; a stopped run keeps them all until Send.
  queue: () => {
    const st = S.queue, list = S.queued, typing = "Add it to my calendar too";
    const row = (q, i) => `<div style="display:flex;align-items:center;gap:8px;min-height:36px;min-width:0">
      <span style="width:18px;flex:none;text-align:center;font-size:12px;font-weight:600;color:var(--m-muted)">${i + 1}</span>
      <div style="flex:1;min-width:0;font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis">${q}</div>
      <button class="ib" style="width:36px;height:36px;color:var(--m-muted)" aria-label="Edit queued message ${i + 1}" data-act="queueEdit:${i}">${I.x}</button>
    </div>`;
    const strip = !list.length ? "" : `<div style="margin:0 14px -2px;padding:4px 4px 2px 12px;border-radius:var(--radius);background:var(--m-surface);border:1.5px solid var(--m-soft);display:grid">
      <div style="display:flex;align-items:center;gap:8px;min-height:30px">
        <span style="color:var(--m-muted);display:grid">${I.clock.replace('width="20" height="20"', 'width="16" height="16"')}</span>
        <div class="label" style="flex:1">${st === "stopped" ? "Not sent" : list.length > 1 ? "Send one by one when Beep finishes" : "Sends when Beep finishes"}</div>
        ${st === "stopped" ? `<button class="ib" style="width:36px;height:36px;color:var(--m-accent)" aria-label="Send queued messages" data-act="queueSend">${I.play}</button>` : ""}
      </div>
      <div style="display:grid;grid-template-columns:minmax(0,1fr);max-height:116px;overflow:auto;scrollbar-width:none">${list.map(row).join("")}</div>
    </div>`;
    const running = st !== "stopped";
    const input = st === "typing" ? `<div class="input" style="color:var(--m-ink)">${typing}</div>` : `<div class="input">Message</div>`;
    const buttons = running
      ? `<div style="display:grid;gap:8px">${st === "typing" ? `<button class="round chunky" aria-label="Queue message" data-act="queueAdd">${I.clock}</button>`
        : ""}<button class="round" style="background:var(--m-ink);color:var(--m-page)" aria-label="Stop reply"><span style="width:14px;height:14px;border-radius:3px;background:currentColor"></span></button></div>`
      : `<button class="round coral" aria-label="Talk">${I.mic}</button>`;
    return topBar() + `<div class="body">
      <div class="user">Did the landlord reply about the boiler?</div>
      ${running ? `<div class="sub" style="padding-left:4px"><span class="spin"></span>Fastmail: read thread</div>`
        : `<div class="reply">Alex replied this morning: the plumber can come Thursday between 9 and 12.</div>`}
    </div>` + (st === "stopped" ? `<div style="margin:4px 16px;padding:12px 14px;border-radius:var(--radius);background:color-mix(in srgb, var(--m-error) 10%, transparent);font-size:13px;color:var(--m-error)">Stopped</div>` : "")
      + strip + `<div class="composer" style="align-items:flex-end">${input}${buttons}</div>`;
  },
  rerun: () => {
    const st = S.rerun, ask = "What's a good way to ask for the deposit back?";
    const pick = MODELS.find(m => m[0] === S.rerunModel) || MODELS[1];
    const act = (icon, label, extra = "") => `<button class="text-btn" style="color:var(--m-muted);font-size:11px;min-height:36px;padding:0 4px;display:flex;align-items:center;gap:6px"${extra}>${icon}${label}</button>`;
    const actions = `<div style="display:flex;align-items:center;margin-top:-6px;position:relative">
      ${act(I.copy, "Copy reply")}${act(I.quote, "Quote")}
      ${S.admin ? act(I.play.replace('width="16" height="16"', 'width="14" height="14"'), "Rerun " + I.chev, ` data-act="rerunMenu" aria-haspopup="menu" aria-expanded="${st === "open"}"`) : ""}
      ${S.admin && (st === "open" || st === "thinking") ? `<div class="menu" role="menu" style="left:130px;top:36px;min-width:210px">
        ${MODELS.map(([id, name]) => { const used = id === S.model, held = st === "thinking" && id === S.rerunHeld; return `<button role="menuitem" data-rerun="${id}" data-rerun-hold="${id}" style="display:grid;grid-template-columns:1fr auto;align-items:center;column-gap:8px;${used ? "background:color-mix(in srgb, var(--m-brand) 12%, transparent);color:var(--m-brand)" : ""}"${used ? ' aria-current="true"' : ""}><span style="${used ? "font-weight:600" : ""}">${esc(name.replace(" (default)", ""))}</span><span style="grid-row:span 2;color:var(--m-brand)">${used ? I.check : ""}</span><span class="${used ? "" : "muted"}" style="font-size:11px">${used ? "Used for this reply" : id === MODELS[0][0] ? "Default" : esc(id)}</span></button>${held ? `<div style="padding:2px 12px 10px;display:grid;gap:6px"><span class="muted" style="font-size:11px">Thinking for this rerun</span><div style="display:flex;gap:4px">${["Off", "Low", "Medium", "High"].map(k => `<button class="fchip" style="min-height:30px;padding:0 9px;font-size:12px" data-rerun-think="${k}">${k}</button>`).join("")}</div></div>` : ""}`; }).join("")}
      </div>` : ""}
      ${S.admin && S.agents ? `<span style="font-size:11px;color:var(--m-dim);padding-left:8px" title="The specialized agent that answered; General shows nothing">Inbox and admin</span>` : ""}
      ${S.admin ? `<span style="font-size:11px;color:var(--m-dim);padding-left:8px" title="Jev's estimate that this turn held something worth remembering">38%</span>` : ""}
    </div>`;
    const first = `<div class="user">${ask}</div>
      <div class="reply">Keep it short and specific. Say when you moved out, that the flat was left in good condition, and ask for the full deposit by a date two weeks out. Attach the move-out photos.</div>`;
    const again = st === "running" || st === "done" ? `<div style="display:grid;justify-items:end;gap:4px"><div class="user">${ask}</div>
      <span class="muted" style="font-size:11px">Rerun on ${esc(pick[1].replace(" (default)", ""))}${S.rerunThinking ? " · " + S.rerunThinking.toLowerCase() : ""}</span></div>` : "";
    const second = st === "running" ? `<div class="sub" style="padding-left:4px"><span class="spin"></span>Beep is thinking…</div>`
      : st === "done" ? `<div class="reply">Write a short email: the date you handed back the keys, that the flat was left clean and undamaged, and the amount you expect back by a set date. Attach the inventory and your move-out photos so there is nothing to dispute.</div>` : "";
    return topBar() + `<div class="body">${first}${st === "running" || st === "done" ? "" : actions}${again}${second}${st === "done" ? actions.replace(' aria-expanded="true"', ' aria-expanded="false"') : ""}</div>` + composer();
  },
  selquote: () => topBar() + `<div class="body">
    <div class="user">What's a good way to ask for the deposit back?</div>
    <div class="reply" style="position:relative;margin-top:46px">Keep it short and specific. <span class="sel">Say when you moved out, that the flat was left in good condition</span>, and ask for the full deposit by a date two weeks out. Attach the move-out photos.
      <div class="seltb" style="left:20px;top:-46px">
        <button data-toast="Copied">Copy</button>
        <button data-toast="Quoted the selection">Quote</button>
        <button data-toast="Selected all">Select all</button>
      </div>
    </div>
  </div>` + composer(),
  bubble: () => `<div style="position:absolute;inset:0;background:linear-gradient(160deg,#DCE6F2,#C9D6E8);"></div>
    <div style="position:relative;padding:20px 18px;color:#2A3646;font-size:13px;display:grid;gap:10px;opacity:.7">
      <b style="font-size:18px">Inbox</b><div>Alex: Plumber Thursday?</div><div>Renfe: Your ticket to Girona</div><div>Bank: Statement ready</div>
    </div>
    <div class="chunky" style="position:absolute;left:12px;right:12px;bottom:12px;top:150px;border-radius:var(--radius);background:var(--m-page);display:flex;flex-direction:column;overflow:hidden;box-shadow:0 4px 0 var(--m-ledge)">
      <div style="height:48px;display:flex;align-items:center;padding:0 4px 0 16px;gap:8px;flex:none">
        <div class="title" style="font-size:18px">${S.bubbleRun === "group" ? "Tokyo trip" : "Beep"}</div>${S.bubbleRun === "group" ? "" : '<span class="dot"></span>'}<div style="flex:1"></div>
        <button class="ib" style="width:40px;height:40px" data-go="drawer" aria-label="Chat options">${I.dots}</button>
      </div>
      <div class="body" style="font-size:14px;gap:10px">
        <div class="user" style="font-size:14px;max-width:230px">Reply to Alex that Thursday morning works</div>
        ${S.bubbleRun === "group" ? `<div class="gm"><div class="b" style="font-size:14px"><span class="who" style="color:#B8574B">Sam</span><span class="mention">@beep</span> can you find us a ryokan for the 14th to the 17th?</div></div>
        <div class="gm beep"><span class="who" style="color:var(--m-brand)">Beep</span><div class="b" style="font-size:14px;line-height:20px">Two quiet ones near Asakusa: Sadachiyo and Kamogawa Asakusa. Want me to hold one?</div></div>`
        : S.bubbleRun === "working" ? `<div class="reply" style="font-size:14px;line-height:20px">I'll find the thread first.</div>
        <div style="display:grid;margin-top:-4px"><div class="sub" style="font-size:13px;padding-left:4px"><span class="spin"></span>Doodling…</div>
          <div class="thoughts" style="margin:8px 0 0">Alex asked about Thursday. <b>Read her last message</b> before writing, in case the time changed.</div></div>`
        : `<div class="chip" style="background:var(--m-t0);align-self:flex-start"><span class="ini">F</span>Fastmail: send email<span class="st" style="color:var(--m-ok)">${I.check}</span></div>
        <div class="reply" style="font-size:14px;line-height:20px">Sent. I told Alex Thursday between 9 and 12 works and thanked her for sorting it.</div>`}
      </div>
      <div class="composer" style="padding:8px 10px 10px 4px"><button class="ib" style="width:42px;height:42px;flex:none;color:var(--m-muted)" data-go="${S.bubbleRun === "group" ? "groupChat" : "chat"}" aria-label="Open full app">${I.expand}</button><div class="input" style="min-height:42px;font-size:14px">Message</div><button class="round coral" style="width:42px;height:42px" aria-label="${S.bubbleRun === "group" ? "Send" : "Talk"}">${S.bubbleRun === "group" ? I.send : I.mic}</button></div>
    </div>
    <div style="position:absolute;right:16px;top:92px;width:52px;height:52px;border-radius:50%;background:var(--m-accent);border:1.5px solid var(--m-ledge);display:grid;place-items:center;font-family:var(--display);font-weight:800;font-size:20px">${BI(24)}</div>`,
  // The Flip closed: the app on the outside screen (about 350 x 340 dp), or the Flex Window widget that opens it.
  cover: () => `<div style="position:absolute;inset:0;background:#1A1F1D;display:grid;place-items:center;padding:16px">
    <div style="width:100%;max-width:330px;display:grid;gap:10px;justify-items:center">
      <div style="width:100%;aspect-ratio:352/339;border-radius:var(--radius);border:6px solid #050807;background:var(--m-page);overflow:hidden;display:flex;flex-direction:column;position:relative">
        <div style="position:absolute;left:18px;top:12px;display:flex;gap:6px;z-index:1"><span style="width:16px;height:16px;border-radius:50%;background:#050807"></span><span style="width:16px;height:16px;border-radius:50%;background:#050807"></span></div>
        ${S.cover === "widget" ? `<div style="flex:1;display:flex;flex-direction:column;justify-content:center;align-items:center;gap:14px;padding:24px;cursor:pointer" data-set="cover:chat" role="button" aria-label="Ask Beep">
          <div style="width:56px;height:56px;border-radius:var(--radius);background:#0B0E1D;color:#fff;border:1.5px solid var(--m-ledge);box-shadow:0 3px 0 var(--m-ledge);display:grid;place-items:center;font-family:var(--display);font-weight:800;font-size:26px">${BI(30)}</div>
          <div class="title" style="font-size:22px">Beep</div>
          <div class="chunky" style="display:flex;align-items:center;gap:8px;padding:10px 18px;border-radius:999px;font-size:15px;color:var(--m-muted)">${I.mic}Ask Beep</div>
        </div>` : `<div style="height:44px;display:flex;align-items:center;padding:0 2px 0 64px;gap:8px;flex:none">
          <div class="title" style="font-size:17px">Beep</div><span class="dot"></span><div style="flex:1"></div>
          <button class="ib" style="width:40px;height:40px" data-go="drawer" aria-label="Chat options">${I.dots}</button>
        </div>
        <div class="body" style="padding:4px 12px 8px;gap:8px">
          <div class="user" style="font-size:13px;line-height:18px;max-width:200px;padding:7px 11px">Do I need an umbrella today?</div>
          <div class="reply" style="font-size:13px;line-height:19px">Yes, rain until about 11:00. It clears after lunch.</div>
        </div>
        <div class="composer" style="padding:4px 10px 10px"><div class="input" style="min-height:40px;font-size:13px">Message</div><button class="round coral" style="width:40px;height:40px" aria-label="Talk">${I.mic}</button></div>`}
      </div>
      <div class="hint" style="color:#9AA8A1">${S.cover === "widget" ? "Flex Window widget. Tap opens Beep on the cover screen." : "Beep on the cover screen. Opening the phone continues the same chat."}</div>
    </div>
  </div>`,
  callout: () => {
    const c = S.callout, err = c.kind === "error";
    const text = err ? "The model stopped responding. Try again."
      : "Yes, Alex replied this morning: the plumber can come Thursday between 9 and 12, and she asked if that...";
    return `<div style="position:absolute;inset:0;background:linear-gradient(160deg,#DCE6F2,#C9D6E8);"></div>
    <div style="position:relative;padding:20px 18px;color:#2A3646;font-size:13px;display:grid;gap:10px;opacity:.7">
      <b style="font-size:18px">Inbox</b><div>Alex: Plumber Thursday?</div><div>Renfe: Your ticket to Girona</div><div>Bank: Statement ready</div><div>Gym: Class moved to 19:00</div>
    </div>
    <div class="fhead ${c.side}" data-go="bubble" aria-label="Open the panel"><span>${BI(22)}</span></div>
    ${c.shown ? `<div class="callout ${c.side}" id="callout" role="button" tabindex="0" aria-label="Open the reply" data-go="bubble">
      <div class="who"><span class="dot${err ? " off" : ""}"></span>Boiler and Saturday weather</div>
      <div class="txt">${text}</div>
      <div class="life"><i></i></div>
    </div>` : `<div class="hint" style="position:absolute;left:0;right:0;bottom:24px">Callout gone. Replay to show it again.</div>`}`;
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
  newRoutine: () => {
    const t = REPEAT_TITLES(), time = `${pad(S.hour)}:${pad(S.minute)}`;
    const days = [[1, "M", "Mon"], [2, "T", "Tue"], [3, "W", "Wed"], [4, "T", "Thu"], [5, "F", "Fri"], [6, "S", "Sat"], [0, "S", "Sun"]];
    const picked = days.filter(([n]) => S.days.includes(n)).map(d => d[2]);
    const summary = { Daily: `Every day at ${time}`, Weekdays: `Weekdays at ${time}`, Weekly: picked.length ? `${picked.join(", ")} at ${time}` : "Pick at least one day",
      Hourly: `Every hour at :${pad(S.minute)}`, Custom: "Runs on the cron schedule, in your timezone" }[S.repeat];
    return R.routines() + `<div class="dialog-wrap"><div class="dialog">
      <h3>New routine</h3>
      <label class="fl">Name<input class="field" id="f-rname" placeholder="Morning briefing"></label>
      <div class="chips">${Object.keys(t).map(k => `<button class="fchip" aria-pressed="${k === S.repeat}" data-repeat="${k}">${t[k]}</button>`).join("")}</div>
      ${S.repeat === "Weekly" ? `<div style="display:flex;gap:4px">${days.map(([n, s, f]) => `<button class="fchip day" aria-label="${f}" aria-pressed="${S.days.includes(n)}" data-day="${n}">${s}</button>`).join("")}</div>` : ""}
      ${S.repeat === "Custom" ? `<label class="fl">Cron expression<input class="field" id="f-cron" placeholder="0 9 * * *" style="font-family:var(--mono)"></label>` :
        `<button class="pill" style="align-self:flex-start" data-act="time">${S.repeat === "Hourly" ? `At :${pad(S.minute)} past the hour` : `At ${time}`}</button>`}
      <div style="font-size:12px;color:var(--m-muted)">${summary}</div>
      <label class="fl">What should Beep do?<textarea class="field" id="f-rprompt" rows="3" placeholder="Give me a summary of..."></textarea></label>
      <button class="text-btn" style="align-self:flex-start;padding:0" data-go="empty" data-toast="New chat: Set up a routine: ">Or just tell Beep in a chat</button>
      <div class="end"><button class="text-btn" data-go="routines">Cancel</button><button class="go" data-go="routines" data-toast="Routine created">Create</button></div>
    </div></div>`;
  },
  memories: () => {
    const groups = [
      ["Identity", [["Speaks Catalan, Spanish and English.", "Sep 30", 92]]],
      ["People", [["Landlord is Alex; the flat's boiler is on her side to fix.", "today", 88]]],
      ["Plan", [["Flies back to Barcelona on Friday the 9th.", "Oct 3", 90]]],
      ["Preference", [["Prefers short, practical answers.", "today", 96], ["Vegetarian, eats fish occasionally.", "Sep 12", 67]]],
      ["Routine", [["Takes the train to Girona most Fridays.", "Sep 28", 74], ["Waters the plants on Mondays and Thursdays.", "Sep 21", 81]]],
      ["Work", [["Works as a product designer at a small studio.", "Sep 10", 85]]],
      ["Setup", [["Has the Android SDK and one emulator; builds go in /srv/project/out.", "today", 90, "m"], ["Old Docker images fill the disk; prune before big builds.", "Oct 8", 77, "m"],
        ["Pushes as the work account, never the personal one.", "Sep 30", 80, "a"], ["Runs the tests with just test, not plain cargo test.", "Oct 3", 86, "p"]]],
    ];
    // Facts a read of mail or a calendar added or confirmed. The "Beep got to know you"
    // notice opens the newest read's chip; All shows everything again.
    const reads = [["read", "Gmail · 10 Oct", 6], ["read2", "Outlook calendar · 4 Oct", 3]];
    const fromRead = [
      ["Identity", [["Speaks Catalan, Spanish and English.", "today", 92]]],
      ["People", [["Landlord is Alex; the flat's boiler is on her side to fix.", "today", 88]]],
      ["Plan", [["Flies back to Barcelona on Friday the 9th.", "today", 90]]],
      ["Preference", [["Buys running shoes from Decathlon, size 43.", "today", 71]]],
      ["Routine", [["Takes the train to Girona most Fridays.", "today", 74]]],
      ["Work", [["Works as a product designer at a small studio.", "today", 85]]],
    ];
    const reading = S.memories === "read", ms = reading ? "all" : (S.mscope || "all");
    const keep = sc => ms === "all" || (ms === "you" ? !sc : sc === ms);
    const shownGroups = (reading ? fromRead : groups).map(([g, list]) => [g, list.filter(x => keep(x[3]))]).filter(([, l]) => l.length);
    const chips = `<div class="chips" style="flex:none"><button class="fchip" aria-pressed="${!reading}" data-act="memList">All</button>${reads.map(([k, label, n]) =>
      `<button class="fchip" aria-pressed="${reading && k === "read"}" data-act="memRead">${label} <span class="muted">${n}</span></button>`).join("")}</div>`;
    const clear = S.memories === "empty" ? "" : `<button class="text-btn" style="color:var(--m-error)" data-act="forget">Clear all</button>`;
    let out = back("Memories", clear) + `<div class="body" style="gap:8px">`;
    if (S.memories === "empty") out += `<div class="empty">Nothing remembered yet.<br>Tell Beep something worth keeping and it shows up here.</div>`;
    else out += `<div class="field" style="display:flex;align-items:center;gap:8px;color:var(--m-dim);flex:none">${I.search}Search memories</div>` + chips +
      (reading ? "" : mScopeChips(groups, "data-set", "mscope")) +
      (reading ? `<div class="muted" style="font-size:13px;line-height:18px;padding:0 4px">What Beep learned or confirmed reading 12 years of Gmail today.</div>` : "") +
      shownGroups.map(([g, list]) => `<div class="label" style="padding:8px 4px 0">${g}</div>` + list.map(([t, d, sc, scope]) => `<div class="card row" style="padding:12px 12px 12px 14px">
        <div class="grow"><span style="font-size:14px;line-height:20px">${t}</span>${scope ? mScopeTag(scope) : ""}<span class="muted" style="font-size:12px">${d}, memory ${sc}%</span></div>
        <button class="ib muted" style="width:36px;height:36px" data-act="edit" aria-label="Edit">${I.edit}</button>
        <button class="ib muted" style="width:36px;height:36px" data-toast="Forgotten" aria-label="Delete">${I.trash}</button>
      </div>`).join("")).join("");
    out += `</div>`;
    if (S.memories === "edit") out += `<div class="dialog-wrap"><div class="dialog"><h3>Edit memory</h3>
      <textarea class="field" id="f-mem" rows="3">Prefers short, practical answers.</textarea>
      <div class="end"><button class="text-btn" data-act="memList">Cancel</button><button class="go" data-act="memList" data-toast="Saved">Save</button></div></div></div>`;
    if (S.memories === "forget") out += `<div class="dialog-wrap"><div class="dialog"><h3>Forget everything?</h3>
      <div class="muted">Beep will forget all 8 memories. This cannot be undone.</div>
      <div class="end"><button class="text-btn" data-act="memList">Cancel</button><button class="go" style="background:var(--m-error);color:#fff" data-act="memEmpty">Forget all</button></div></div></div>`;
    return out;
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
  settings: () => {
    const opt = k => `<button class="setrow" data-bubble="${k}" style="padding:10px 4px"><div class="grow"><b>${k}</b></div>${S.bubbleStyle === k ? `<span>${I.check}</span>` : ""}</button>`;
    return back("Settings") + `<div class="body" style="gap:2px">
      <button class="setrow" data-toast="Key saved. Keep the file private."><span class="logo" style="background:${TINTS[2]}">${I.key}</span><div class="grow"><b>alex</b><span>Key backed up to your Google account</span></div></button>
      <button class="setrow" data-go="devices"><span class="logo" style="background:${TINTS[0]}">${I.laptop}</span><div class="grow"><b>Linked devices</b><span>Use Beep from your computer</span></div></button>
      ${S.admin ? `<button class="setrow" data-go="machines"><span class="logo" style="background:${TINTS[1]}">${I.server}</span><div class="grow"><b>Machines</b><span>Where Beep runs commands and coding agents</span></div></button>` : ""}
      <div class="hr" style="margin:10px 4px"></div>
      <div class="hr" style="margin:10px 4px"></div>
      <div class="label" style="padding:4px">Floating chat</div>
      ${["Beep head", "Bubble", "Off"].map(opt).join("")}
      <div class="hr" style="margin:10px 4px"></div>
      <div class="label" style="padding:4px">Voice speed</div>
      <div class="chips" style="padding:0 4px">${VOICE_SPEEDS.map(k => `<button class="fchip" aria-pressed="${S.voiceSpeed === k}" data-vspeed="${k}">${k}</button>`).join("")}</div>
      <div class="muted" style="font-size:12px;padding:4px 4px 0">How fast Beep talks in live mode.</div>
      ${S.admin ? `<div class="hr" style="margin:10px 4px"></div>
      <div class="label" style="padding:4px;display:flex;gap:8px;align-items:center">Model <span class="tag">Admin</span></div>
      <div class="setrow"><span class="logo" style="background:${TINTS[2]}">${I.brain}</span><div class="grow"><b>Auto</b><span>${S.auto ? (S.agents ? "Jev picks an agent, which brings its prompt and thinking, on the model below" : "Jev picks the thinking for each prompt, on the model below") : (S.agents ? "Use the model below, with the agent's prompt" : "Off: every prompt uses the model below")}</span></div><button class="switch" role="switch" aria-checked="${S.auto}" aria-label="Auto" data-act="auto"></button></div>
      ${`<div style="display:flex;flex-direction:column;gap:8px;padding:4px 0">${MODELS.map(([id, name, inp, out, text]) => `<button class="radio" role="radio" aria-checked="${S.model === id}" data-model="${id}"><span class="o"></span><div class="grow"><b style="font-weight:500">${name}</b><div class="muted" style="font-size:12px">$${inp} in, $${out} out per M tokens${text ? ", text only" : ""}</div></div></button>`).join("")}</div>
      <div class="label" style="padding:10px 4px 4px">Thinking</div>
      <div class="chips" style="padding:0 4px">${["Off", "Low", "Medium", "High"].map(k => `<button class="fchip" aria-pressed="${S.thinking === k}" data-thinking="${k}">${k}</button>`).join("")}</div>`}
      <div class="muted" style="font-size:12px;padding:0 4px">${S.agents ? "Applies to your own turns." : "Applies to your own turns. Everyone else stays on the default."}</div>
      <div class="hr" style="margin:10px 4px"></div>
      <div class="label" style="padding:4px;display:flex;gap:8px;align-items:center">Agents <span class="tag">Admin</span></div>
      <div class="setrow"><span class="logo" style="background:${TINTS[1]}">${I.brain}</span><div class="grow"><b>Agents</b><span>${S.agents ? "On for everyone: Jev picks an agent for each prompt" : "Off: everyone uses the general assistant"}</span></div><button class="switch" role="switch" aria-checked="${S.agents}" aria-label="Agents" data-act="agents"></button></div>
      <div style="display:flex;flex-direction:column;gap:2px;padding:2px 0;opacity:${S.agents ? 1 : 0.5}">${AGENTS.map(([name, when, model]) => `<div class="setrow" style="padding:6px 4px"><div class="grow"><b style="font-weight:500">${name}</b><span>${when}</span></div><span class="muted" style="font-size:12px;text-align:right;white-space:nowrap">${model}</span></div>`).join("")}</div>` : ""}
    </div>`;
  },
  devices: () => {
    const none = S.devices === "none", joined = S.devices === "joined";
    const dev = (icon, tint, name, note, end) => `<div class="setrow"><span class="logo" style="background:${tint}">${icon}</span><div class="grow"><b>${name}</b><span>${note}</span></div>${end}</div>`;
    const owner = `<span class="tag" style="background:var(--m-t1)">Owner</span>`;
    const rows = joined
      ? dev(I.laptop, TINTS[0], "MacBook", "Desktop, macOS. Active 2 min ago", owner) + dev(I.phone, TINTS[1], "Pixel", "This phone", "")
      : dev(I.phone, TINTS[1], "Pixel", "This phone", owner)
        + (none ? "" : dev(I.laptop, TINTS[0], "MacBook", "Desktop, macOS. Active 2 min ago", `<button class="text-btn" style="color:var(--m-error)" data-act="removeDesk">Remove</button>`)
          + dev(I.server, TINTS[2], "build-box", "Machine, linked 3 days ago", `<button class="text-btn" style="color:var(--m-error)" data-toast="build-box removed">Remove</button>`));
    const note = (joined ? "This phone is on MacBook's account. " : none ? "No computers linked yet. " : "A linked device sees your chats and memories. ")
      + "Any device on the account can add another. Phone actions such as contacts and alarms stay on this phone.";
    const actions = `<div style="flex:1"></div>
      <button class="primary" style="display:flex;align-items:center;justify-content:center;gap:8px" data-go="linkConfirm" data-toast="Camera opens to scan">${I.laptop}Scan a computer's code</button>
      <button class="pill chunky" style="margin-top:10px;display:flex;align-items:center;justify-content:center;gap:8px;font-weight:600;min-height:48px" data-go="showCode">${I.phone}Show a code for a new device</button>
      <button class="text-btn" style="align-self:center;margin-top:6px" data-go="typeCode">Type a code instead</button>`;
    let out = `<div class="bar" style="padding-left:6px"><button class="ib" data-go="settings" aria-label="Back">${I.back}</button><div class="title" style="flex:1">Linked devices</div></div>
    <div class="body" style="gap:2px">
      <div class="label" style="padding:4px">Devices on this account</div>
      ${rows}
      <div class="hr" style="margin:10px 4px"></div>
      <div class="muted" style="font-size:12px;line-height:18px;padding:0 4px">${note}</div>
      ${actions}
    </div>`;
    if (S.devices === "removing") out += `<div class="dialog-wrap"><div class="dialog"><h3>Remove MacBook?</h3>
      <div class="muted">It is signed out at once and can no longer see your chats or memories.</div>
      <div class="end"><button class="text-btn" data-act="devCancel">Cancel</button><button class="go" style="background:var(--m-error);color:#fff" data-act="devRemove">Remove</button></div></div></div>`;
    return out;
  },
  // Any device shows a code; the phone or computer that scans it joins this account.
  showCode: () => R.devices() + `<div class="scrim" data-go="devices" style="z-index:8"></div>
    <div class="sheet" role="dialog" aria-label="Show a code">
      <div class="grab"></div>
      <h3>Add a device</h3>
      <div class="muted" style="font-size:13px;line-height:19px">On the new phone or computer, scan this in Beep: Settings, Linked devices, or Using Beep on a computer on iPhone. Its chats and memories move to this account.</div>
      <div class="qr" style="justify-self:center;align-self:center;margin:0 auto;width:180px;height:180px">${qrSvg()}</div>
      <div class="field" style="display:flex;align-items:center;justify-content:center;height:56px;font-family:var(--mono);font-size:26px;font-weight:600;letter-spacing:3px;color:var(--m-ink)">K7QM4-X9TRP</div>
      <div class="muted" style="font-size:12px;text-align:center">A code lasts ten minutes.</div>
      <div class="end" style="justify-content:space-between"><button class="text-btn" style="padding:0" data-toast="Code copied">Copy code</button><button class="go" style="box-shadow:0 3px 0 var(--m-ledge)" data-go="devices">Done</button></div>
    </div>`,
  // A typed join code leads to the same sheet as a scanned one.
  typeCode: () => {
    const was = S.devices; S.devices = "linked";
    const under = S.onb ? R.permissions() : R.devices(); S.devices = was;
    const back = S.onb ? "permissions" : "devices";
    return under + `<div class="dialog-wrap"><div class="dialog" role="dialog" aria-label="Type the code"><h3>Type the code</h3>
      <input class="field" id="joinCodeField" style="font-family:var(--mono);letter-spacing:.12em" placeholder="K7QM4-X9TRP" autocomplete="off" spellcheck="false">
      <div class="muted" style="font-size:12px;margin-top:-6px">The code your other device shows under Settings, Devices, or the link beep-node link prints.</div>
      <div class="end"><button class="text-btn" data-go="${back}">Cancel</button><button class="go" data-go="joinConfirm">Next</button></div></div></div>`;
  },
  // Shown after scanning or typing a join code from a computer that has its own account.
  joinConfirm: () => {
    const was = S.devices; S.devices = "linked";
    const under = S.onb ? R.permissions() : R.devices(); S.devices = was;
    const back = S.onb ? "permissions" : "devices";
    return under + `<div class="scrim" data-go="${back}"></div>
    <div class="sheet" role="dialog" aria-label="Join MacBook's account">
      <div class="grab"></div>
      <div class="row" style="align-items:center"><span class="logo" style="width:52px;height:52px;border-radius:var(--radius);background:var(--m-t0)">${I.laptop}</span>
        <div class="grow"><h3>Join MacBook's account?</h3><span class="muted" style="font-size:13px">Desktop, macOS</span></div></div>
      <div style="font-size:15px;line-height:22px">This phone's chats, memories, routines and places move to MacBook's account, and from then on both show the same chats. Apps you connected on this phone may need connecting again.</div>
      <div class="end"><button class="text-btn" data-go="${back}">Cancel</button><button class="go" style="box-shadow:0 3px 0 var(--m-ledge)" data-act="joinDo">Join</button></div>
    </div>`;
  },
  linkConfirm: () => {
    const was = S.devices; S.devices = "none";
    const under = S.onb ? R.permissions() : R.devices(); S.devices = was;
    return under + `<div class="scrim" data-act="linkCancel"></div>
    <div class="sheet" role="dialog" aria-label="Link MacBook">
      <div class="grab"></div>
      <div class="row" style="align-items:center"><span class="logo" style="width:52px;height:52px;border-radius:var(--radius);background:var(--m-t0)">${I.laptop}</span>
        <div class="grow"><h3>Link MacBook?</h3><span class="muted" style="font-size:13px">Desktop, macOS</span></div></div>
      <div style="font-size:15px;line-height:22px">It will see your chats and memories.</div>
      <div class="card" style="padding:10px 14px;display:grid;grid-template-columns:auto 1fr;gap:4px 14px;font-size:13px">
        <span class="muted">Code</span><span style="font-family:var(--mono);letter-spacing:.1em">7K4 PQ2</span>
        <span class="muted">Stays here</span><span>Contacts, alarms, location</span></div>
      <div class="muted" style="font-size:12px">Check that the code matches the one on the computer.</div>
      <div class="end"><button class="text-btn" data-act="linkCancel">Cancel</button><button class="go" style="box-shadow:0 3px 0 var(--m-ledge)" data-act="linkDo">Link</button></div>
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
const IC = {
  up: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="M12 19V5M5 12l7-7 7 7"/></svg>',
  stop: '<span style="width:12px;height:12px;border-radius:3px;background:currentColor;display:block"></span>',
  image: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="3"/><circle cx="9" cy="10" r="2"/><path d="M21 16l-5-5-9 9" stroke-linecap="round"/></svg>',
  ne: '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M7 17L17 7M8 7h9v9"/></svg>',
  gt: '<svg width="8" height="14" viewBox="0 0 8 14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M1 1l6 6-6 6"/></svg>',
  pinf: '<svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor"><path d="M15 3l6 6-3 1-4 4 1 5-2 2-4-4-5 5-1-1 5-5-4-4 2-2 5 1 4-4z"/></svg>',
  send: '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M21 3L10 14M21 3l-7 18-4-7-7-4 18-7z"/></svg>',
};

const P = {};
S.i = { online: true, run: "done", thoughts: "closed", fold: false, typing: false, approve: "ask", chats: "list", admin: true, model: "z-ai/glm-5.3-flash", level: "Low", auto: true, agents: false, mem: "list", swipe: "", on: [true, true, true, false], repeat: "Weekdays", perms: { loc: false, notif: false }, imported: false };
// The iPhone group screens are the Android ones: same bar, bubbles, sheets and
// group info. Their links are moved to the iPhone screens.
const I_GO = { drawer: "iChats", chat: "iChat", empty: "iEmpty", groupChat: "iGroup", groupInfo: "iGroupInfo", groupInvite: "iGroupInvite",
  groupNew: "iGroupNew", groupCode: "iGroupCode", groupJoin: "iGroupJoin" };
const iGroup = html => html.replace(/data-go="(\w+)"/g, (_, to) => `data-igo="${I_GO[to] || "iGroup"}"`)
  .replace(/data-toast=/g, "data-itoast=").replace(/data-act="/g, 'data-gact="').replace(/data-set="/g, 'data-gset="');
