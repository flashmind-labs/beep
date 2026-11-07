// ---- Chat list home ----
// The app opens on a list like a messenger: Beep pinned on top, then Beep's other chats and the groups, newest first.
// No unread counts and no mute. Memories, routines, places and apps move to the menu.
const OFTEN = { always: "Runs on every visit", daily: "Skips it if you come back the same day", once: "Runs the next time, then deletes itself" };
Object.assign(S, { home: "list", save: "saved", radius: "100", places: "list", trig: "arrive", trigPlace: "Gym", trigRepeat: "daily", arrive: "arrived", bgLoc: "ask", thread: "runs", ginfo: "info", creator: true });
// Opening Beep or one of its chats: back to the list, Beep's avatar, the chat's name.
function chatBar() { return topBar(); }
const toolChip = (label, ini, bad) => `<div class="chip" style="background:var(--m-t1);align-self:flex-start"><span class="ini">${ini}</span>${label}<span class="st" style="color:var(${bad ? "--m-error" : "--m-ok"})">${bad ? I.x : I.check}</span></div>`;

Object.assign(R, {
  placeSave: () => {
    const st = S.save;
    const head = chatBar() + `<div class="body"><div class="user">I'm at the gym</div>`;
    if (st === "off") return head + toolChip("Location: here", "L", true)
      + `<div class="reply">I can't see where you are, location is off for Beep. Turn it on and say it again, and I'll save the spot.</div>
      <div class="banner">${I.pin}<span style="flex:1">Location is off for Beep</span><button class="pill" style="min-height:34px;padding:0 12px" data-go="locAlways">Turn on</button></div></div>` + composer();
    if (st === "near") return head + toolChip("Location: here", "L") + toolChip("Places: list", "P")
      + `<div class="reply">You already have <b>Gym</b> saved, 40 m from here. Move it to where you are now?</div>
      <div style="display:flex;gap:6px 8px;flex-wrap:wrap"><button class="chunky rchip" data-set="save:saved" data-toast="Gym moved">Move Gym here</button>
        <button class="chunky rchip" data-toast="Sent: save it as a new place">Save as a new place</button><button class="chunky rchip" data-toast="Sent: leave it">Leave it</button></div></div>` + composer();
    return head + toolChip("Location: here", "L") + toolChip("Places: save Gym", "P")
      + `<div class="reply">Saved this spot as <b>Gym</b>. I'll notice when you get here or leave, so you can ask for things like "when I get to the gym, remind me to stretch".</div>
      ${placeCard()}</div>` + composer();
  },
  places: () => {
    const st = S.places;
    const bar = `<div class="bar" style="padding-left:6px"><button class="ib" data-go="chat" aria-label="Back">${I.back}</button><div class="title" style="flex:1">Places</div></div>`;
    const hint = `<div class="card" style="font-size:13px;line-height:19px;background:var(--m-t1)">Tell Beep where you are to add one, like <b>"I'm at the gym"</b> or <b>"this is home"</b>.</div>`;
    if (st === "empty") return bar + `<div class="body"><div class="empty" style="padding:48px 20px 16px">No places yet.<br>Beep can remind you when you arrive somewhere, or leave.</div>${hint}</div>`;
    const rows = PLACES.map(([n, addr, r, used, last, bg]) => `<div class="card row" style="align-items:center;padding:12px 14px;border-width:2px;border-radius:var(--radius);box-shadow:3px 3px 0 var(--m-ledge)">
      <span class="logo" style="background:${bg}">${I.pin}</span>
      <div class="grow"><b style="font-weight:600;font-size:15px">${n}</b><span class="muted" style="font-size:12.5px">${addr}</span>
        <span style="font-size:11.5px;color:var(--m-dim)">Within ${r} · ${used} · ${last}</span></div>
      <button class="ib" style="width:36px;height:36px;color:var(--m-muted)" data-set="places:delete" aria-label="Delete ${n}">${I.x}</button></div>`).join("");
    const noloc = st === "noloc" ? `<button class="banner" style="text-align:left;color:var(--m-ink)" data-go="locAlways">${I.pin}<span style="flex:1">Beep can't notice arrivals yet. Allow location all the time.</span>${IC.gt}</button>` : "";
    const del = st === "delete" ? `<div class="dialog-wrap"><div class="dialog"><h3>Delete Gym?</h3>
      <div class="muted">Log my weights runs when you arrive here, and stops. Beep forgets this place.</div>
      <div class="end"><button class="text-btn" data-set="places:list">Cancel</button><button class="go" style="background:var(--m-error);color:#fff" data-set="places:list" data-toast="Gym deleted">Delete</button></div></div></div>` : "";
    return bar + `<div class="body" style="gap:10px">${noloc}${rows}${hint}</div>` + del;
  },
  // A routine can start on a time, or when you arrive at or leave a saved place.
  // How often a place routine starts, as the schedule's ending says: nothing, :daily or :once.
  placeRoutine: () => {
    const st = S.trig;
    if (st === "chat") return chatBar() + `<div class="body">
      <div class="user">When I get to the gym, remind me to log my weights</div>
      ${toolChip("Places: find Gym", "P")}${toolChip("Routines: create", "R")}
      <div class="reply">Done. Next time you arrive at <b>Gym</b> I'll remind you, once a day at most.</div>
      <div class="card row" style="align-items:center;border-width:2px;box-shadow:3px 3px 0 var(--m-ledge);border-radius:var(--radius)">
        <span class="logo" style="background:var(--m-t0)">${I.pin}</span>
        <div class="grow"><b style="font-weight:600;font-size:14px">Log my weights</b><span class="muted" style="font-size:12px;line-height:17px">When you arrive at Gym · once a day at most</span></div>
        <button class="switch" role="switch" aria-checked="true" aria-label="On" data-toast="Paused"></button></div>
    </div>` + composer();
    const place = st !== "time", leave = st === "leave";
    const chip = (k, l) => `<button class="fchip" aria-pressed="${st === k}" data-set="trig:${k}">${l}</button>`;
    const often = (k, l) => `<button class="fchip" aria-pressed="${S.trigRepeat === k}" data-set="trigRepeat:${k}">${l}</button>`;
    const where = place ? `<div class="chips">${PLACES.map(([n]) => `<button class="fchip" aria-pressed="${(leave ? "Home" : S.trigPlace) === n}" data-set="trigPlace:${n}">${I.pin.replace('width="20" height="20"', 'width="13" height="13"').replace("<svg", '<svg style="vertical-align:-2px;margin-right:4px"')}${n}</button>`).join("")}</div>
      <div class="fl">How often<div class="chips">${often("always", "Every time")}${often("daily", "Once a day")}${often("once", "Next time only")}</div>
        <span class="muted" style="font-size:12px">${OFTEN[S.trigRepeat]}</span></div>`
      : `<button class="pill" style="align-self:flex-start">Every day at 18:00</button>`;
    return R.routines() + `<div class="dialog-wrap"><div class="dialog">
      <h3>New routine</h3>
      <label class="fl">Name<input class="field" value="${leave ? "Heating check" : "Log my weights"}"></label>
      <div class="fl">When<div class="chips">${chip("time", "At a time")}${chip("arrive", "I arrive")}${chip("leave", "I leave")}</div></div>
      ${where}
      <label class="fl">What should Beep do?<textarea class="field" rows="3">${leave ? "Check the heating is off and tell me if rain is coming." : "Remind me to log today's weights and show what I lifted last time."}</textarea></label>
      <div class="end"><button class="text-btn" data-go="routines">Cancel</button><button class="go chunky" style="border-radius:var(--radius)" data-go="routines" data-toast="Routine created">Create</button></div>
    </div></div>`;
  },
  // Arriving at a place starts the routine as a new chat. It shows as a notification and at the top of the list.
  arrive: () => {
    const st = S.arrive;
    if (st === "opened") return chatBar("Log my weights", "Started when you arrived at Gym") + `<div class="body">
      ${iDayA("Today, 18:04")}
      <div class="sub" style="align-self:center">${I.pin.replace('width="20" height="20"', 'width="14" height="14"')}You arrived at Gym</div>
      <div class="reply">Time to log today's weights. Last Thursday you did:<ul style="margin:6px 0;padding-left:20px"><li>Squats 3x5 at 80 kg</li><li>Bench press 3x5 at 60 kg</li><li>Rows 3x8 at 50 kg</li></ul>Tell me what you lift today and I'll keep the list.</div>
      <div style="display:flex;gap:6px 8px;flex-wrap:wrap"><button class="chunky rchip" data-toast="Sent: same as last time">Same as last time</button><button class="chunky rchip" data-toast="Sent: skip today">Skip today</button></div>
    </div>` + composer();
    const [t, b] = st === "left" ? ["Heating check", "You left Home. The heating is still on and rain starts at 16:00."] : ["Log my weights", "You're at Gym. Time to log today's weights. Last time: squats 3x5 at 80 kg."];
    return chatHtml() + `<div class="notif" data-set="arrive:opened" data-go="arrive" style="cursor:pointer">
      <span class="logo" style="width:36px;height:36px;border-radius:var(--radius);background:#0B0E1D;color:#fff">${BI(20)}</span>
      <div class="grow"><b style="font-size:14px">${t}</b><span style="font-size:13px;color:var(--m-muted);line-height:18px">${b}</span></div>
      <span style="font-size:11px;color:var(--m-dim)">now</span></div>`;
  },
  // Asked the first time a place routine is made. Without it, place routines only run while the app is open.
  locAlways: () => {
    const st = S.bgLoc, ios = true;
    const always = ios ? "Always" : "Allow all the time";
    const row = (icon, t) => `<div class="row">${icon}<span>${t}</span></div>`;
    if (st === "on") return `<div class="body" style="justify-content:center;align-items:center;text-align:center;gap:14px;padding:24px">
      <span class="logo" style="width:64px;height:64px;border-radius:var(--radius);background:var(--m-t1);color:var(--m-ok)">${I.check}</span>
      <div class="big" style="font-size:30px">Beep will notice when you arrive.</div>
      <div class="muted" style="max-width:270px">Your places work with the app closed. Turn it off any time in ${ios ? "Settings, Beep, Location" : "Settings, Apps, Beep, Location"}.</div>
      <button class="primary" style="margin-top:12px" data-go="places">See your places</button></div>`;
    const steps = st === "settings" ? `<div class="card" style="display:grid;gap:8px;font-size:13px;line-height:19px">
        <b style="font-size:14px">${ios ? "iOS asks for this in Settings" : "Android asks for this in Settings"}</b>
        <span>1. Tap <b>Open settings</b></span><span>2. ${ios ? "Location" : "Permissions, then Location"}</span><span>3. Choose <b>${always}</b></span></div>` : "";
    return `<div class="body" style="padding:28px 20px 16px;gap:16px">
      <span class="logo" style="width:56px;height:56px;border-radius:var(--radius);background:var(--m-t0)">${I.pin}</span>
      <div class="big">Notice when you arrive?</div>
      <div class="muted" style="line-height:22px">For routines like "when I get to the gym", the phone watches your saved places and tells Beep when you arrive or leave. That needs location set to <b style="color:var(--m-ink);font-weight:600">${always}</b>.</div>
      <div class="card" style="display:grid;gap:10px;font-size:13px;line-height:19px">
        ${row(I.pin, "Works with the app closed, only for places you saved.")}
        ${row(I.bell, "You only get a notification when a routine has something to say.")}
        ${row(I.shield, "Uses the phone's geofencing, so the battery cost is small.")}
      </div>${steps}
      <div style="flex:1"></div>
      <button class="primary" data-set="bgLoc:${st === "settings" ? "on" : "settings"}">${st === "settings" ? "Open settings" : always}</button>
      <button class="text-btn" style="align-self:center" data-go="chat">Not now</button></div>`;
  },

  // ---- Group runs and replies ----
  // Messages in a row from one person share a name and tighter corners. A reply carries a chip with the quoted message; tapping it jumps there.
  groupThread: () => {
    const st = S.thread, jump = st === "jump", sam = "#B8574B";
    const q = (who, color, text) => `<button class="qchip" data-set="thread:jump" aria-label="Go to the quoted message"><b style="color:${color}">${who}</b><span>${text}</span></button>`;
    const sm = (html, first, cls = "") => `<div class="gm${cls}"><div class="b">${first ? `<span class="who" style="color:${sam}">Sam</span>` : ""}${html}</div></div>`;
    const reply = st === "reply" ? `<div class="replybar"><div class="grow" style="gap:0"><b style="font-size:12px;color:${sam}">Replying to Sam</b><span class="muted" style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis">Somewhere near Asakusa? Not too fancy</span></div>
      <button class="ib" style="width:32px;height:32px;color:var(--m-muted)" data-set="thread:runs" aria-label="Cancel reply">${I.x}</button></div>` : "";
    return groupBar() + `<div class="body" style="gap:14px;padding:8px 18px 16px">
      ${iDayA("Today")}
      <div class="grun">${sm("Ryokan is sorted, I paid the deposit", true)}${sm("Now dinner on the 14th")}${sm("Somewhere near Asakusa? Not too fancy", false, jump ? " flash" : "")}</div>
      <div class="grun me"><div class="user">Not sushi again pls</div><div class="user">and somewhere we can walk to</div></div>
      ${gMsg("Beep", "b", "var(--m-brand)", "var(--m-brand)", `${q("Sam", sam, "Somewhere near Asakusa? Not too fancy")}Three within a ten minute walk of the ryokan, none of them sushi:<br><br><b>Asakusa Imahan</b>: sukiyaki, book ahead.<br><b>Daikokuya</b>: tempura, no bookings.<br><b>Kappo Ohashi</b>: small and seasonal.`)}
      <div class="grun">${sm(`${q("Beep", "var(--m-brand)", "Asakusa Imahan: sukiyaki, book ahead.")}Imahan! <span class="mention">@beep</span> book it for 20:00`, true)}</div>
    </div>` + reply + groupComposer();
  },

  // Group info: a header with the group's initials and name, members you can tap, and Leave at the bottom.
  // Only the person who made the group can rename it.
  groupInfo: () => {
    const st = S.ginfo, mine = S.creator;
    const members = [["You", "D", "var(--m-t2)", mine ? "Dani · made the group" : "Dani · joined 3 Oct", mine], ["Sam", "S", "var(--m-t0)", mine ? "Joined 2 Oct" : "Made the group", !mine]];
    const bar = `<div class="bar" style="padding-left:6px"><button class="ib" data-go="groupChat" aria-label="Back">${I.back}</button><div class="title" style="flex:1">Group info</div>
      <button class="ib muted" data-go="groupInvite" aria-label="Invite">${I.link}</button></div>`;
    let out = bar + `<div class="body">
      <div class="ghead">${av("TT", "var(--m-t0)", "width:84px;height:84px;font-size:28px;border-width:2px;box-shadow:0 3px 0 var(--m-ledge)")}
        <div style="display:flex;align-items:center;gap:2px"><div class="title" style="font-size:28px">Tokyo trip</div>
          ${mine ? `<button class="ib" style="width:36px;height:36px;color:var(--m-muted)" data-set="ginfo:rename" aria-label="Rename">${I.edit}</button>` : ""}</div>
        <div class="muted" style="font-size:13px">Group · 3 members</div></div>
      <div class="label">Members</div>
      <div class="card" style="display:grid;gap:12px;padding:12px 14px">
        ${members.map(([n, i, bg, d, made]) => `<button class="mrow" data-set="ginfo:${n === "Sam" ? "member" : "info"}">${av(i, bg, "width:36px;height:36px;font-size:13px")}
          <div class="grow"><b style="font-weight:500;display:flex;align-items:center;gap:6px">${n}${made ? '<span class="tag" style="background:var(--m-t1);font-size:10px">Made it</span>' : ""}</b><span class="muted" style="font-size:12px">${d}</span></div>
          ${n === "You" ? "" : `<span style="color:var(--m-dim);display:grid">${IC.gt}</span>`}</button>`).join("")}
        <div class="row" style="align-items:center">${av("b", "var(--m-brand)", "width:36px;height:36px;font-size:13px")}<div class="grow"><b style="font-weight:500">Beep</b><span class="muted" style="font-size:12px">Joins in when it can help, or when someone writes @beep</span></div></div>
        <button class="pill chunky" style="display:flex;align-items:center;justify-content:center;gap:8px;font-weight:600" data-go="groupInvite">${I.link}Invite with a code</button>
      </div>
      <div class="label">Reminders</div>
      ${groupReminders()}
      <div class="label">Your apps</div>
      <div class="card" style="display:grid;gap:8px;padding:12px 14px">
        <div class="row" style="align-items:center"><div class="grow"><b style="font-weight:500">Share my apps with Beep here</b><span class="muted" style="font-size:12px;line-height:17px">${S.shareApps ? "Beep can read your calendar, mail, files and tasks to plan for the group, and never shows them here. Creating or sending anything asks you first." : "Beep uses your apps here only when you ask it yourself."}</span></div>
        <button class="switch" role="switch" aria-checked="${S.shareApps}" aria-label="Share my apps with Beep here" data-act="shareApps"></button></div>
      </div>
      <div class="label">Privacy</div>
      <div class="card" style="display:grid;gap:10px;font-size:13px;line-height:19px">
        <div class="row">${I.brain}<span>Beep reads every member's memories but never quotes them here. Memories from email and Slack stay out of groups.</span></div>
        <div class="row">${I.shield}<span>Beep's thinking and tool calls are hidden in groups. Members only see its replies.</span></div>
      </div>
      <button class="dangerbtn" data-set="ginfo:leave">${I.exit}Leave group</button>
    </div>${S.gRem === "deleted" ? `<div class="snack">Reminder deleted</div>` : ""}`;
    if (st === "rename") out += `<div class="dialog-wrap"><div class="dialog"><h3>Rename group</h3>
      <input class="field" value="Tokyo and Kyoto">
      <div class="muted" style="font-size:13px;line-height:19px">Everyone sees the new name. Only you can change it, since you made the group.</div>
      <div class="end"><button class="text-btn" data-set="ginfo:info">Cancel</button><button class="go" data-set="ginfo:info" data-toast="Renamed">Save</button></div></div></div>`;
    if (st === "member") out += `<div class="scrim" data-set="ginfo:info" style="z-index:8"></div><div class="sheet" role="dialog" aria-label="Sam">
      <div class="grab"></div>
      <div class="row" style="align-items:center">${av("S", "var(--m-t0)", "width:56px;height:56px;font-size:20px")}
        <div class="grow"><h3>Sam</h3><span class="muted" style="font-size:13px">${mine ? "Joined 2 Oct with your code" : "Made the group on 1 Oct"}</span></div></div>
      <div class="card" style="display:grid;gap:10px;font-size:13px;line-height:19px">
        <div class="row">${I.apps}<span style="flex:1">Shares apps with Beep here</span><b style="font-weight:600">No</b></div>
        <div class="row">${I.clock}<span style="flex:1">Reminders set</span><b style="font-weight:600">2</b></div></div>
      <button class="pill chunky" style="display:flex;align-items:center;justify-content:center;gap:8px;font-weight:600" data-go="mention" data-set="ginfo:info">@ Mention Sam</button>
    </div>`;
    if (st === "leave") out += `<div class="dialog-wrap"><div class="dialog"><h3>Leave Tokyo trip?</h3>
      <div class="muted" style="font-size:14px;line-height:20px">You stop getting its messages. To come back, someone has to send you the code again.${mine ? " Sam becomes the one who can rename it." : ""}</div>
      <div class="end"><button class="text-btn" data-set="ginfo:info">Cancel</button><button class="go" style="background:var(--m-error);color:#fff" data-set="ginfo:info" data-go="chat" data-toast="Left Tokyo trip">Leave</button></div></div></div>`;
    return out;
  },
});
const PLACES = [
  ["Home", "Carrer de Verdi 31, Gràcia", "150 m", "1 routine", "here now", "var(--m-t1)"],
  ["Gym", "Carrer de Pujades 102, Poblenou", "100 m", "1 routine", "last here Thu", "var(--m-t0)"],
  ["Office", "Plaça de Catalunya 9", "100 m", "no routines", "last here Fri", "var(--m-t2)"],
  ["Mum's", "Carrer Major 4, Sant Cugat", "250 m", "no routines", "last here 28 Sep", "var(--m-t1)"],
];
function placeCard() {
  const r = S.radius, px = { 50: 44, 100: 72, 250: 124 }[r];
  return `<div class="card" style="padding:0;overflow:hidden;flex-shrink:0">
    <div class="map"><span class="ring" style="width:${px}px;height:${px}px"></span><span class="pinm">${I.pin.replace('width="20" height="20"', 'width="16" height="16"')}</span><small>&copy; OpenStreetMap</small></div>
    <div style="padding:14px;display:grid;gap:3px">
      <div style="display:flex;align-items:center;gap:8px"><div class="title" style="font-size:17px;flex:1">Gym</div><span class="tag" style="background:var(--m-t1)">Saved</span></div>
      <div class="muted" style="font-size:13px">Carrer de Pujades 102, Poblenou</div>
      <div class="label" style="margin-top:10px">Counts as here within</div>
      <div class="chips">${["50", "100", "250"].map(k => `<button class="fchip" aria-pressed="${r === k}" data-set="radius:${k}">${k} m</button>`).join("")}</div>
      <div style="display:flex;flex-wrap:wrap;gap:10px;margin-top:10px">
        <button class="cbtn chunky" style="background:var(--m-accent)" data-go="placeRoutine" data-set="trig:arrive">${I.clock.replace('width="20" height="20"', 'width="16" height="16"')}When I get here</button>
        <button class="cbtn chunky" data-toast="Rename Gym">${I.edit}Rename</button></div>
    </div></div>`;
}
const vs = (key, list) => list.map(([label, val]) => [label, () => { S[key] = val; render(); }, () => S[key] === val]);
Object.assign(VARIANTS, {
  placeSave: vs("save", [["Saved", "saved"], ["Already saved nearby", "near"], ["Location off", "off"]]).concat(vs("radius", [["50 m", "50"], ["250 m", "250"]])),
  places: vs("places", [["List", "list"], ["Delete", "delete"], ["Location not always", "noloc"], ["Empty", "empty"]]),
  placeRoutine: vs("trig", [["Arrive", "arrive"], ["Leave", "leave"], ["At a time", "time"], ["From chat", "chat"]]),
  arrive: vs("arrive", [["Arrived", "arrived"], ["Left home", "left"], ["Opened", "opened"]]),
  locAlways: vs("bgLoc", [["Ask", "ask"], ["Settings", "settings"], ["Allowed", "on"]]),
  groupThread: vs("thread", [["Runs and replies", "runs"], ["Tapped a quote", "jump"], ["Replying", "reply"]]),
  groupInfo: vs("ginfo", [["Info", "info"], ["Rename", "rename"], ["Member", "member"], ["Leave", "leave"]])
    .concat([["Not the creator", () => { S.creator = !S.creator; render(); }, () => !S.creator]])
    .concat(vs("gRem", [["Deleted reminder", "deleted"], ["No reminders", "empty"]])),
});
// The iPhone gets the same list, places and group screens.
Object.assign(I_GO, { home: "iChats", share: "iShare", shared: "iShared", appDetail: "iAppDetail", mcpAdd: "iMcpAdd", mcpDetail: "iMcpDetail", placeSave: "iPlaceSave", places: "iPlaces", placeRoutine: "iPlaceRoutine", arrive: "iArrive", locAlways: "iLocAlways",
  groupThread: "iGroupThread", settings: "iSettings", memories: "iMemories", routines: "iRoutines", chat: "iChat", mention: "iGroup", apps: "iApps" });
IOS_SCREENS.find(g => g.group === "Chat").items.splice(IOS_SCREENS.find(g => g.group === "Chat").items.findIndex(x => x[0] === "iChats") + 1, 0,
  ["iShare", "Share a chat", "ShareChatSheet"], ["iShared", "Open a shared chat", "SharedChatView"]);
IOS_SCREENS.splice(IOS_SCREENS.findIndex(g => g.group === "Settings") + 1, 0, { group: "Apps", items: [
  ["iApps", "Apps", "AppsView"], ["iAppDetail", "App permissions", "AppPermissionsView"],
  ["iMcpAdd", "Add MCP server", "AddMcpServerSheet"], ["iMcpDetail", "MCP server", "McpServerView"],
]});
IOS_SCREENS.find(g => g.group === "Group chats").items.splice(1, 0, ["iGroupThread", "Runs and replies", "GroupChatView, replies"]);
IOS_SCREENS.splice(IOS_SCREENS.length - 1, 0, { group: "Places", items: [
  ["iPlaceSave", "I'm at the gym", "PlaceCard"], ["iPlaces", "Places", "PlacesView"], ["iPlaceRoutine", "Place routine", "NewRoutineView, place"],
  ["iArrive", "Arriving somewhere", "Arrival notification"], ["iLocAlways", "Location always", "Region monitoring permission"],
]});
[["iChats", "drawer"], ["iShare", "share"], ["iShared", "shared"], ["iApps", "apps"], ["iAppDetail", "appDetail"], ["iMcpAdd", "mcpAdd"], ["iMcpDetail", "mcpDetail"], ["iPlaceSave", "placeSave"], ["iPlaces", "places"], ["iPlaceRoutine", "placeRoutine"], ["iArrive", "arrive"], ["iLocAlways", "locAlways"], ["iGroupThread", "groupThread"]].forEach(([i, a]) => {
  P[i] = () => iGroup(R[a]()); VARIANTS[i] = VARIANTS[a];
});
VARIANTS.iGroupInfo = VARIANTS.groupInfo;
