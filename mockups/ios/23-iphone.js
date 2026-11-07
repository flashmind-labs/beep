// ---- iPhone ----
// The glow around the screen edge stays while live mode is on; it never takes a tap.
let lvCard = "";
iLiveView = o => {
  const lv = S.i.lv, f = LV_FROM[lv.from], launch = o.launch && !lv.rm;
  const press = launch && f.press ? `<div class="press" style="${f.side}:10px">${f.press}</div>` : "";
  lvCard = o.card || "";
  return `<div class="ilive docked${lv.rm ? " rm" : ""}${launch ? " launch" : ""}" aria-hidden="true" style="--ox:${f.ox};--oy:${f.oy};--odx:${f.odx};--ody:${f.ody}">
    <div class="edge"></div><div class="edge sharp"></div><div class="wash"></div><div class="spark"></div>${press}</div>`
    + lvDock({ ...o, rm: lv.rm, launch }, {
      orb: () => 'data-iact="lvTap"',
      leave: mid => `data-lvleft="${mid ? "cancel" : "draft"}"`,
    });
};
const lvUser = t => `<div class="user" style="border-width:2px;box-shadow:3px 3px 0 var(--m-ledge);border-radius:var(--radius) 18px 6px 18px;margin-left:48px">${t}</div>`;
const LV_CAL = "What’s on my calendar tomorrow morning?";
const LV_CAL_REPLY = "You have the dentist at 9:30, then a call with Alex at 11 about the plumber.";
/// The turn live mode is on, drawn in the chat like any other.
function iLiveNow() {
  const caret = '<span class="caret"></span>';
  switch (S.screen) {
    case "iLiveThink": {
      const v = S.i.lv.think;
      if (v === "says") return lvUser("Is build-box running out of disk?") + iChip("Running code", "S", "run", 0) + `<div class="reply">Let me check build-box.${caret}</div>`;
      return lvUser(LV_CAL) + (v === "tool" ? iChip("Checking Google Calendar", "G", "run", 1) : "") + `<div class="reply">${caret}</div>`;
    }
    case "iLiveSpeak":
      return lvUser(LV_CAL) + iChip("Checked Google Calendar", "G", "ok", 1) + `<div class="reply">${LV_CAL_REPLY}</div>`;
    case "iLiveAsk":
      return lvUser("Tell Alex Thursday morning works") + (S.i.lvAsk === "done"
        ? `<div class="muted" style="font-size:12px">Allowed once</div>` + iChip("Sending email", "F", "run", 0) + `<div class="reply">${caret}</div>`
        : lvCard);
    default: return "";
  }
}
for (const id of ["iLiveLaunch", "iLive", "iLiveThink", "iLiveSpeak", "iLiveAsk"]) {
  const base = P[id];
  P[id] = () => {
    let html = base().replace(">New conversation<", ">Plumber and dentist<");
    const at = html.indexOf('<div class="icomp">'), end = at < 0 ? -1 : html.lastIndexOf("</div>", at);
    if (end >= 0) html = html.slice(0, end) + iLiveNow() + html.slice(end);
    return html;
  };
}
// Dark was the full screen scene's look; the dock follows the chat's theme.
for (const id of ["iLiveLaunch", "iLive", "iLiveThink", "iLiveSpeak", "iLiveAsk"]) VARIANTS[id] = VARIANTS[id].filter(v => v[0] !== "Dark");
S.i.lv.dark = false;
{
  const g = IOS_SCREENS.find(g => g.group === "Live voice").items;
  const name = { iLiveLaunch: "LiveDock, EdgeGlow, TalkToBeepIntent", iLive: "LiveDock over ChatView, listening", iLiveThink: "LiveDock, thinking", iLiveSpeak: "LiveDock, speaking", iLiveAsk: "LiveDock, ApprovalCard in the chat" };
  g.forEach(it => { if (name[it[0]]) it[2] = name[it[0]]; });
}

// The orb in a dock follows a made up level: speech while listening, playback while speaking.
let dockRaf = 0;
function dockLoop(root) {}
ilvLoop = function () {
  cancelAnimationFrame(ilvRaf);
  const scene = document.querySelector("#ios .ilive");
  const phone = document.querySelector(".iphone");
  phone.classList.add("withbtn");
  if (!phone.querySelector(".sidebtn")) { const b = document.createElement("span"); b.className = "sidebtn"; b.style.top = "200px"; phone.append(b); }
  if (!phone.querySelector(".actbtn")) { const b = document.createElement("span"); b.className = "actbtn"; b.style.top = "150px"; phone.append(b); }
  const launching = !!scene?.classList.contains("launch");
  phone.classList.toggle("pressed", launching);
  phone.classList.toggle("act", launching && S.i.lv.from === "action");
  phone.classList.toggle("side", launching && S.i.lv.from === "side");
  dockLoop(document.getElementById("ios"));
};
