// ---- Talk to Beep widget and the live session's Live Activity (BeepWidget target) ----
// The widget's button runs the same Talk to Beep intent as the Action Button: the app opens straight
// into live mode with the launch glow (from the middle, as for a control). It has no reply to show,
// since the widget cannot read the app's chats, so both sizes are just the button.
// While a live session runs, a Live Activity shows its state with a small orb in the Dynamic Island
// and on the Lock Screen; a tap goes back to the session, End leaves live mode, and it ends with it.
(() => {
  const st = document.createElement("style");
  st.textContent = ``;
  document.head.append(st);
})();
S.i.tw = { act: "compact", state: "listen" };

// Settings: a card on putting live voice on the Action Button, the controls, and when a pause sends.
S.i.lvPause = true;
// Live voice speed, one setting per device; both phones share it here.
const VOICE_SPEEDS = ["1×", "1.25×", "1.5×", "2×"];
S.voiceSpeed = "1.25×";
document.addEventListener("click", e => { const b = e.target.closest("[data-vspeed]"); if (b) { S.voiceSpeed = b.dataset.vspeed; render(); } });

// ---- Tool labels (ToolLabels.kt): every tool says what it is doing while it runs and what it did once
// done. Tools that take an action (memory, skill, background, habit, reminder) label each action.
const TOOL_LABELS = [
  ["M", "Recalling memories", "Recalled memories"],
  ["M", "Remembering", "Remembered"],
  ["M", "Forgetting a memory", "Forgot a memory"],
  ["S", "Loading a skill", "Loaded a skill"],
  ["S", "Saving a skill", "Saved a skill"],
  ["B", "Starting an agent", "Started an agent"],
  ["B", "Running a job", "Ran a job"],
  ["B", "Checking background work", "Checked background work"],
  ["B", "Messaging an agent", "Messaged an agent"],
  ["H", "Logging a habit", "Logged a habit"],
  ["C", "Drawing a chart", "Drew a chart"],
  ["R", "Planning a route", "Planned a route"],
];
S.toolLabels = S.toolLabels || "run";
R.toolLabels = () => {
  const done = S.toolLabels === "done";
  return topBar() + `<div class="body">
    <div class="user">What do you know about my week, and can you look into flights to Girona?</div>
    ${TOOL_LABELS.map(([ini, run, past], i) => `<div class="chip" style="background:${TINTS[i % 3]};align-self:flex-start"><span class="ini${done ? "" : " wiggle"}">${ini}</span>${done ? past : run}<span class="st">${done ? `<span style="color:var(--m-ok);display:grid">${I.check}</span>` : '<span class="spin"></span>'}</span></div>`).join("")}
  </div>` + composer();
};
SCREENS.find(g => g.group === "Chat").items.splice(SCREENS.find(g => g.group === "Chat").items.findIndex(i => i[0] === "recall") + 1, 0,
  ["toolLabels", "Tool labels", "ToolChip, ToolLabels"]);
VARIANTS.toolLabels = vs("toolLabels", [["Running", "run"], ["Done", "done"]]);

// A background question answered on another device, or withdrawn when the work timed out or was
// stopped, folds to one line here unless this phone answered it. Opening the chat reads the waiting
// ones from the server (ListQuestions), so cards survive restarts and show on every device.
const approveBg = R.approve;
R.approve = () => {
  if (S.approve !== "bgdone") return approveBg();
  S.approve = "resolved";
  const html = approveBg();
  S.approve = "bgdone";
  return html.replace(/<div class="chip"[\s\S]*?<\/span><\/div>/, "")
    .replace(/<div class="user">[^<]*<\/div>/, `<div class="user">Find a slot with Alex next week and send her an invite</div>
      <div class="reply">I started an agent on it in the background.</div>`);
};
VARIANTS.approve.push(["Background, no longer waiting", () => { S.approve = "bgdone"; render(); }, () => S.approve === "bgdone"]);

// ---- Routine grants (GrantsScreen): what routines may do without asking, from ListGrants. A grant
// always ends: 8 hours after it was given unless the agent asked for longer, at most a week. Settings
// opens every grant; a routine with grants opens just its own. Revoke asks first, then RevokeGrant.
const GRANTS = [
  { routine: "Inbox sweep", ini: "G", app: "Gmail", action: "reply to thread", ends: "Ends in 5 h", used: "Last used 40 min ago",
    fields: [["Allows", "1 reply each time it runs, without asking"], ["To", "ana@example.com only"], ["Only in", "this thread"], ["Subject", "Re: Saturday plans"]] },
  { routine: "Weekly review", ini: "F", app: "Fastmail", action: "send email", ends: "Ends in 6 d", used: "Not used yet",
    fields: [["Allows", "Up to 2 emails each time it runs, without asking"], ["To", "alex@example.com only"]] },
];
S.grants = S.grants || "list";
const grantCard = (g, i) => `<div class="card" style="display:grid;gap:10px">
    <div class="row" style="align-items:center"><span class="logo" style="width:32px;height:32px;font-size:13px;background:${TINTS[i % 3]}">${g.ini}</span>
      <div class="grow"><b style="font-weight:600;font-size:14px">${g.routine}</b><span class="muted" style="font-size:12px">${g.app}: ${g.action}</span></div></div>
    <div style="display:grid;grid-template-columns:62px 1fr;gap:4px 8px;font-size:13px;padding:10px;border-radius:var(--radius);background:var(--m-page)">
      ${g.fields.map(([k, v]) => `<span class="muted" style="font-size:12px">${k}</span><span>${v}</span>`).join("")}</div>
    <div class="row" style="align-items:center"><div class="grow" style="gap:0">
      <span style="font-size:12px;color:var(--m-brand);font-weight:500">${g.ends}</span>
      <span style="font-size:12px;color:var(--m-muted)">${g.used}</span></div>
      <button class="text-btn" style="color:var(--m-error)" data-set="grants:${i ? "confirm2" : "confirm"}">Revoke</button></div>
  </div>`;
R.grants = () => {
  const st = S.grants, one = st === "one";
  const shown = st === "empty" ? [] : st === "revoked" ? GRANTS.slice(1) : one ? GRANTS.slice(0, 1) : GRANTS;
  const bar = `<div class="bar" style="padding-left:6px"><button class="ib" data-go="${one ? "routines" : "settings"}" aria-label="Back">${I.back}</button><div class="title" style="flex:1">Routine grants</div></div>`;
  const body = shown.length ? shown.map((g, i) => grantCard(g, GRANTS.indexOf(g))).join("")
    + `<div class="hint" style="line-height:18px">${one ? "Only the grants of Inbox sweep." : "Routines do these without asking. Each one ends on its own, and anything else still asks."}</div>`
    : `<div class="empty">No routine grants.<br>When a routine needs to act without asking, Beep asks you once in a chat.</div>`;
  const ask = st === "confirm" || st === "confirm2" ? GRANTS[st === "confirm" ? 0 : 1] : null;
  return bar + `<div class="body" style="gap:8px">${body}</div>` + (ask ? `<div class="dialog-wrap"><div class="dialog">
      <h3>Revoke this grant?</h3>
      <div class="muted" style="font-size:14px;line-height:20px">${ask.routine} will ask before each ${ask.action} again.</div>
      <div class="end"><button class="text-btn" data-set="grants:list">Cancel</button><button class="go" style="background:var(--m-error);color:#fff" data-set="grants:revoked" data-toast="Grant revoked">Revoke</button></div>
    </div></div>` : "");
};
SCREENS.find(g => g.group === "Routines").items.push(["grants", "Routine grants", "GrantsScreen"]);
VARIANTS.grants = vs("grants", [["Every grant", "list"], ["One routine's", "one"], ["Revoke, confirm", "confirm"], ["Revoked", "revoked"], ["Empty", "empty"]]);

// A routine with grants says so under its schedule and opens its own grants.
const routinesGrantBase = R.routines;
R.routines = () => {
  const link = (text, st) => `<button class="text-btn" style="align-self:flex-start;padding:0;min-height:28px;font-size:12px;display:flex;gap:4px;align-items:center" data-set="grants:${st}" data-go="grants">${small(I.shield, 14)}${text}</button>`;
  return routinesGrantBase()
    .replace("Weekdays at 13:30</span>", `Weekdays at 13:30</span>${link("1 grant, ends in 5 h", "one")}`)
    .replace("Every Sunday at 18:00</span>", `Every Sunday at 18:00</span>${link("1 grant, ends in 6 d", "list")}`);
};

// Settings opens every grant, next to linked devices.
const settingsGrantBase = R.settings;
R.settings = () => settingsGrantBase().replace("<span>Use Beep from your computer</span></div></button>",
  `<span>Use Beep from your computer</span></div></button>
      <button class="setrow" data-set="grants:list" data-go="grants"><span class="logo" style="background:${TINTS[1]}">${I.shield}</span><div class="grow"><b>Routine grants</b><span>What routines may do without asking</span></div></button>`);

// The agent asks for a grant with request_grant; it arrives as an ordinary approval card whose action
// ends in "for a routine". Allow saves the grant; there is no Always for it, since a grant always ends.
const approveGrantBase = R.approve;
R.approve = () => {
  if (S.approve !== "grant" && S.approve !== "grantok") return approveGrantBase();
  const ok = S.approve === "grantok";
  const fields = [["Routine", "Inbox sweep"], ["Allows", "1 reply each time it runs, without asking"], ["To", "ana@example.com only"],
    ["Only in", "this thread"], ["Subject", "Re: Saturday plans"], ["Ends", "When the routine ends, or in 8 h"]];
  const card = ok ? `<div class="muted" style="font-size:12px;display:flex;align-items:center;gap:6px;padding:0 4px">${I.shield}Gmail, reply to thread for a routine: Allowed for this routine</div>`
    : `<div class="ask">
      <div class="head"><span class="logo" style="width:32px;height:32px;font-size:13px;background:var(--m-t0)">G</span>
        <div class="grow"><b style="font-weight:600;font-size:14px">Reply to thread for a routine?</b><span class="muted" style="font-size:12px">Gmail asks before acting</span></div>${I.shield}</div>
      <div class="kv" style="grid-template-columns:62px 1fr">${fields.map(([k, v]) => `<span>${k}</span><span>${v}</span>`).join("")}</div>
      <div class="acts"><span style="flex:1"></span>
        <button class="pill" style="min-height:38px;padding:0 14px" data-toast="Denied. The routine asks each time.">Deny</button>
        <button class="go" style="min-height:38px;box-shadow:0 3px 0 var(--m-ledge)" data-set="approve:grantok">Allow</button>
      </div></div>`;
  const status = ok ? `<span style="color:var(--m-ok);display:grid">${I.check}</span>` : '<span class="spin"></span>';
  return topBar() + `<div class="body">
    <div class="user">Whenever Ana replies about Saturday, reply back for me</div>
    <div class="reply">I set up Inbox sweep to watch that thread. To reply without asking each time, it needs your OK once.</div>
    <div class="chip" style="background:var(--m-t1);align-self:flex-start"><span class="ini">R</span>${ok ? "Asked to allow a routine" : "Asking to allow a routine"}<span class="st">${status}</span></div>
    ${card}
    ${ok ? `<div class="reply">Done. Inbox sweep can now send one reply to Ana in that thread each time it runs, for the next 8 hours.</div>` : ""}
  </div>` + composer();
};
VARIANTS.approve.push(["For a routine", () => { S.approve = "grant"; render(); }, () => S.approve === "grant"],
  ["For a routine, allowed", () => { S.approve = "grantok"; render(); }, () => S.approve === "grantok"]);
TOOL_LABELS.push(["R", "Asking to allow a routine", "Asked to allow a routine"]);

try { const s = localStorage.getItem("beep-screens"); if (s && R[s]) S.screen = s; } catch (e) {}
const hash = location.hash.slice(1);
if (R[hash]) S.screen = hash;
render();
