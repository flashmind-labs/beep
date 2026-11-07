// ---- Desktop images, memories and routines ----
// Pasted or dropped images, resized to 1024 px on the long side, and pasted or dropped files (up to 5 MB in all), sent as they are.
// There is no file picker: the bar hides when a dialog takes focus.
const THUMBS = ["linear-gradient(150deg,#C98B5A,#6B3E2A 70%)", "linear-gradient(160deg,#8FB3C9,#3E5F75 65%)", "linear-gradient(140deg,#7FA36B,#2F4A2A 70%)", "linear-gradient(170deg,#D9C27A,#8A6A2E)", "linear-gradient(150deg,#B88AC9,#4E3A66)", "linear-gradient(160deg,#E0A090,#7A4038)", "linear-gradient(150deg,#9AA6AE,#3E474D)", "linear-gradient(140deg,#6FB5A2,#24584B)"];
const dThumbs = (n, sent = false) => `<div class="dthumbs${sent ? " sent" : ""}">${THUMBS.slice(0, n).map((g, i) => `<span class="dthumb" style="background:${g}" role="img" aria-label="Image ${i + 1}">${sent ? "" : `<button aria-label="Remove image ${i + 1}" data-toast="Removed">${DI.x}</button>`}</span>`).join("")}</div>`;
const DOC = '<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round"><path d="M6 2.5h8l4.5 4.5v14.5H6z"/><path d="M14 2.5V7h4.5"/></svg>';
const dAchip = (name, size, sent = false) => `<span class="dachip" role="img" aria-label="${name}"><span class="ic">${DOC}</span><span style="min-width:0"><span class="nm">${name}</span><span class="sz">${size}</span></span>${sent ? "" : `<button aria-label="Remove ${name}" data-toast="Removed">${DI.x}</button>`}</span>`;
const MEMS = [
  ["People", [
    ["Sam is my partner; birthday 14 March", "You told Beep · 2 Sep"],
    ["Alex is the landlord and prefers email to calls", "From your email · 18 Sep", "Please email rather than ring, I rarely pick up."],
  ]],
  ["Preference", [
    ["Vegetarian, but eats fish", "You told Beep · 7 Oct"],
    ["Likes a window seat on trains", "Learned in a chat · 3 Oct"],
  ]],
  ["Work", [
    ["Design review every Thursday at 10:30", "From your email · 22 Sep"],
    ["On the Lisbon project until December", "From Slack · 1 Oct · to 31 Dec"],
    ["Covering for Jo while she is away", "Learned in a chat · 28 Sep · to 4 Oct", "", true],
  ]],
  ["Setup", [
    ["Has the Android SDK and one emulator; builds go in /srv/project/out", "Learned in a chat · 7 Oct", "", false, "m"],
    ["Pushes as the work account, never the personal one", "You told Beep · 30 Sep", "", false, "a"],
    ["Runs the tests with just test, not plain cargo test", "Learned in a chat · 3 Oct", "", false, "p"],
  ]],
];
const ROUTINES = [
  ["Morning briefing", "Every day at 8:00", "Next: tomorrow 8:00", true, "Summarise my calendar, the weather and anything urgent in email."],
  ["Inbox sweep", "Weekdays at 17:30", "Next: today 17:30", true, "Archive newsletters and list what still needs a reply."],
  ["Water the plants", "Mon, Thu at 19:00", "Paused", false, "Remind me to water the plants."],
  ["Renew passport", "Once, 14 Oct 10:00", "", true, "Remind me to book the passport appointment."],
];
Object.assign(D, {
  dAttach: () => {
    const st = S.d.att;
    // The prompt and what went with it sit in "You", as in a coding agent session.
    const youWith = (text, att) => dYou(`<span style="display:grid;gap:8px;min-width:0">${text}${att}</span>`);
    if (st === "sentFile") return deskScene(dBar(D_ASK + `<div class="dbody">${dLog(`${youWith("Anything unusual in this lease?", `<div class="dthumbs sent">${dAchip("Lease 2026.pdf", "PDF, 240 KB", true)}</div>`)}
      ${dDone([["Running code"]], 6)}
      <div class="dcoderen agent md">Two clauses stand out:
      <ul><li><b>Clause 7.2</b>: the rent can rise by up to 6% each year, above the usual cap.</li><li><b>Clause 11</b>: you pay for all repairs under 300 EUR, including the boiler.</li></ul>
      The rest is standard: 30 days' notice, one month's deposit.</div>`)}
    </div>` + dHints()));
    if (st === "files" || st === "big") {
      const err = st === "big" ? `<div class="dmuted" style="color:var(--d-err);padding:6px 16px 0 56px">Files up to 5 MB in one message</div>` : "";
      return deskScene(dBar(`<div class="dthumbs">${THUMBS.slice(0, 1).map((g, i) => `<span class="dthumb" style="background:${g}" role="img" aria-label="Image 1"><button aria-label="Remove image 1" data-toast="Removed">${DI.x}</button></span>`).join("")}${dAchip("Lease 2026.pdf", "PDF, 240 KB")}${dAchip("rent.csv", "CSV, 3 KB")}</div>` + err
        + `<div class="dinput" data-dgo="dStream" style="cursor:text"><span class="txt">Compare the lease with what I paid<span class="caret" style="height:20px"></span></span></div>`
        + dHints([[DI.ret, "send"], ["⌘V", "paste"], ["⌫", "remove last"], ["esc", "to hide"]])));
    }
    if (st === "sent") return deskScene(dBar(D_ASK + `<div class="dbody">${dLog(`${youWith("What plant is this and why are the leaves going brown?", dThumbs(2, true))}
      ${dHead("Worked for 3s")}
      <div class="dcoderen agent md">The first photo is a calathea. Brown, crispy edges usually mean the air is too dry or the water is hard. The second photo shows the soil pulling away from the pot, so it has dried out completely between waterings.
      <ul><li>Water with filtered or rain water once the top 2 cm is dry.</li><li>Move it away from the radiator.</li><li>Trim the brown edges with clean scissors.</li></ul></div>`)}
    </div>` + dHints()));
    const n = st === "one" ? 1 : st === "three" ? 3 : 8;
    const err = st === "max" ? `<div class="dmuted" style="color:var(--d-err);padding:6px 16px 0 56px">Up to 8 images in one message</div>` : "";
    const text = st === "one" ? `<span class="txt">What plant is this?<span class="caret" style="height:20px"></span></span>`
      : `<span class="caret" style="height:22px"></span><span class="txt ph" style="margin-left:-10px">Add a message (optional)</span>`;
    const drop = st === "drop" ? `<div class="ddrop">${IC.image.replace('width="20" height="20"', 'width="18" height="18"')}Drop images or files to ask about them</div>` : "";
    return deskScene(dBar(drop + (st === "drop" ? "" : dThumbs(n) + err) + `<div class="dinput" data-dgo="dStream" style="cursor:text">${st === "drop" ? `<span class="txt ph">Ask Beep…</span>` : text}</div>`
      + dHints([[DI.ret, "send"], ["⌘V", "paste"], ["⌫", "remove last"], ["esc", "to hide"]])));
  },
  // Same data as the phone's Memories. Tab on a row shows Edit and Forget; Return edits in place.
  dMemories: () => {
    const st = S.d.mem;
    const search = st === "search", q = "email";
    const ms = search ? "all" : S.d.mscope || "all", keep = sc => ms === "all" || (ms === "you" ? !sc : sc === ms);
    const total = MEMS.reduce((n, [, l]) => n + l.length, 0);
    const head = n => `<div class="dinput"><span class="txt${search ? "" : " ph"}">${search ? q : "Search memories"}</span><span class="dmuted">${n}</span><kbd>⌘M</kbd></div>`;
    if (st === "empty") return deskScene(dBar(head(0) + `<div style="border-top:1px solid var(--d-border)"></div>
      <div class="dlist" style="padding-top:2px"><div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--d-dim);display:grid">${I.brain}</span><b style="font-size:14px">Nothing remembered yet.</b><span class="dmuted" style="display:block">Beep learns as you chat, or tell it "remember that…"</span></div></div>` + dHints([["↑↓", "move"], [DI.ret, "edit"], ["⌘⌫", "forget"], ["esc", "back"]])));
    let idx = 0;
    const sel = st === "list" || st === "edit" ? (ms === "all" ? 2 : 0) : search ? 0 : -1;
    const hl = t => search ? t.replace(/email/gi, m => `<mark>${m}</mark>`) : t;
    const groups = (ms === "read" ? DREAD : MEMS).map(([cat, list]) => {
      const rows = list.filter(([c, , qt, , sc]) => ms === "read" || (keep(sc) && (!search || (c + cat + (qt || "")).toLowerCase().includes(q)))).map(([c, m, qt, ended, sc]) => {
        const i = idx++, on = i === sel, about = sc ? dScopeTag(sc) + " · " : "";
        if (on && st === "edit") return `<div class="dmem" aria-selected="true"><div class="top"><textarea rows="2" aria-label="Memory">${c}</textarea><span style="font-size:12px;color:var(--d-muted);flex:none;display:inline-flex;gap:4px;align-items:center">${DI.ret} save</span></div><div class="m">${about}${m}</div></div>`;
        const acts = on ? `<span class="acts"><button class="dbtn" data-dact="mem:edit">Edit</button><button class="dbtn warn" data-toast="Forgotten">Forget<kbd>⌘⌫</kbd></button></span>` : "";
        return `<div class="dmem${ended ? " ended" : ""}" role="option" aria-selected="${on}"><div class="top"><span class="c">${hl(c)}</span>${acts}</div><div class="m">${about}${m}</div>${qt ? `<q>${hl(qt)}</q>` : ""}</div>`;
      });
      return rows.length ? `<div class="dsec" style="border:0;padding-left:10px">${cat}</div>${rows.join("")}` : "";
    }).join("");
    const clear = `<div class="dcard" style="margin:8px"><div style="padding:10px 12px;display:grid;gap:8px">
        <b style="font-size:13.5px">Forget everything?</b><div class="dmuted" style="display:block">Beep will forget all ${total} memories. This cannot be undone.</div>
        <div style="display:flex;gap:8px"><button class="dbtn warn" data-dact="mem:empty" data-toast="All memories forgotten">Forget all</button><button class="dbtn quiet" data-dact="mem:list">Cancel</button></div></div></div>`;
    const hints = st === "edit" ? [[DI.ret, "save"], [`⇧${DI.ret}`, "new line"], ["esc", "cancel"]]
      : [["↑↓", "move"], [DI.ret, "edit"], ["⌘⌫", "forget"], ...(search ? [] : [["tab", "filter"]]), ["esc", "back"]];
    return deskScene(dBar(head(search || ms !== "all" ? idx + " of " + total : total) + `<div style="border-top:1px solid var(--d-border)">${search ? "" : dScopeChips()}</div>`
      + `<div class="dlist" role="listbox" aria-label="Memories" style="padding-top:2px;flex:1;min-height:0;overflow:auto;scrollbar-width:none">${st === "clear" ? clear : groups + (search || ms !== "all" ? "" : `<div style="padding:6px 10px 4px"><button class="dbtn warn" data-dact="mem:clear">Forget all…</button></div>`)}</div>` + dHints(hints)));
  },
  // Tab on a row shows Run now and Delete. Run now opens the run in the bar like any chat.
  dRoutines: () => {
    const st = S.d.rt, sel = st === "list" ? 0 : 1;
    const head = `<div class="dhead"><span class="tt"><b>Routines</b><small>${st === "empty" ? "Nothing scheduled" : "3 on, 1 paused"}</small></span><button class="dbtn" data-dgo="dNewRoutine">${I.plus.replace('width="20" height="20"', 'width="12" height="12"')}New<kbd>⌘N</kbd></button></div>`;
    if (st === "empty") return deskScene(dBar(head + `<div style="padding:28px 24px 32px;display:grid;justify-items:center;gap:8px;text-align:center"><span style="color:var(--d-dim);display:grid">${I.clock}</span><b style="font-size:14px">No routines yet.</b><span class="dmuted" style="display:block">Schedule tasks for Beep to run on a timer.</span><button class="dbtn pri" data-dgo="dNewRoutine" style="margin-top:4px">New routine</button></div>` + dHints([["⌘N", "new routine"], ["esc", "back"]])));
    const rows = ROUTINES.map(([label, when, next, on, prompt], i) => {
      const cur = i === sel;
      if (cur && st === "delete") return `<div class="drt" aria-selected="true"><span class="tt"><span style="font-size:14px">Delete <b>${label}</b>?</span></span><span class="acts"><button class="dbtn warn" data-dact="rt:list" data-toast="Deleted">Delete</button><button class="dbtn quiet" data-dact="rt:list">Cancel</button></span></div>`;
      const tail = cur && st === "actions"
        ? `<span class="acts"><button class="dbtn" data-dgo="dStream" data-toast="Running Inbox sweep">${I.play.replace('width="16" height="16"', 'width="10" height="10"')}Run now</button><button class="dbtn warn" data-dact="rt:delete">Delete<kbd>⌘⌫</kbd></button></span>`
        : `<button class="dswitch" role="switch" aria-checked="${on}" aria-label="${label} on" data-toast="${on ? "Paused" : "Resumed"}"></button>`;
      return `<div class="drt${on ? "" : " off"}" role="option" aria-selected="${cur}"><span class="ic">${I.clock}</span><span class="tt"><b>${label}</b><span>${prompt}</span><small>${[when, next].filter(Boolean).join(" · ")}</small></span>${tail}</div>`;
    }).join("");
    const hints = st === "actions" ? [["⌘E", "edit"], ["tab", "hide actions"], ["esc", "back"]] : [["↑↓", "move"], ["space", "on or off"], ["tab", "actions"], ["⌘N", "new routine"], ["⌘G", "grants"], ["esc", "back"]];
    return deskScene(dBar(head + `<div class="dlist" style="padding-top:8px">${rows}</div>` + dHints(hints)));
  },
  // The same choices as the phone's dialog. Weekly shows day chips, Once a date, Cron a raw expression.
  dNewRoutine: () => {
    const st = S.d.nr, rep = st === "err" ? "day" : st === "noday" ? "week" : st;
    const chip = (k, l) => `<button class="dchip" aria-pressed="${rep === k}" data-dact="nr:${k}">${l}</button>`;
    const days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"].map((d, i) => `<button class="dchip day" aria-pressed="${st !== "noday" && (i === 0 || i === 3)}">${d}</button>`).join("");
    const when = rep === "cron" ? `<label class="dfield on"><span>Cron expression</span><span class="f mono" style="letter-spacing:.04em">30 7 * * 1-5</span></label><span class="dmuted" style="margin-top:-6px">Weekdays at 7:30</span>`
      : rep === "once" ? `<div style="display:grid;grid-template-columns:1fr 1fr;gap:10px"><label class="dfield"><span>Date</span><span class="f">Wed 14 Oct</span></label><label class="dfield"><span>Time</span><span class="f">10:00</span></label></div>`
      : `${rep === "week" ? `<div class="dchips">${days}</div>` : ""}<label class="dfield" style="max-width:140px"><span>${rep === "hour" ? "Minute past" : "Time"}</span><span class="f">${rep === "hour" ? "00:00" : "08:00"}</span></label>`;
    const name = st === "once" ? "Renew passport" : st === "week" || st === "noday" ? "Water the plants" : "Morning briefing";
    const task = st === "err" ? "" : st === "once" ? "Remind me to book the passport appointment." : st === "week" || st === "noday" ? "Remind me to water the plants." : "Summarise my calendar, the weather and anything urgent in email.";
    return deskScene(dBar(`<div class="dhead"><span class="tt"><b>New routine</b><small>Runs on this account, phone or computer off</small></span></div>
      <div class="dform" style="padding-top:12px;flex:1;min-height:0;overflow:auto;scrollbar-width:none;align-content:start">
        <label class="dfield${st === "err" ? "" : " on"}"><span>Name</span><span class="f">${st === "err" ? '<span class="ph">Morning briefing</span>' : name}</span></label>
        <label class="dfield${st === "err" ? " err" : ""}"><span>${st === "err" ? "Tell Beep what to do" : "What should Beep do?"}</span><span class="f area">${task || '<span class="ph">Summarise my calendar and the weather</span>'}</span></label>
        <div class="dfield"><span>Repeat</span><div class="dchips">${chip("day", "Every day")}${chip("weekdays", "Weekdays")}${chip("week", "Weekly")}${chip("hour", "Every hour")}${chip("once", "Once")}${chip("cron", "Cron")}</div></div>
        ${when}
        ${st === "noday" ? '<div class="derr">Pick at least one day</div>' : ""}
        <div style="display:flex;gap:8px;align-items:center"><button class="dbtn pri" data-dgo="${st === "err" ? "dNewRoutine" : "dRoutines"}"${st === "err" ? "" : ' data-toast="Routine created"'}>Create<kbd>⌘${DI.ret}</kbd></button><button class="dbtn quiet" data-dgo="dRoutines">Cancel</button><span class="dmuted" style="margin-left:auto">Or just tell Beep in a chat</span></div>
      </div>` + dHints([["tab", "next field"], [`⌘${DI.ret}`, "create"], ["esc", "cancel"]])));
  },
});
