// ---- Talk to Beep widget and the live session's Live Activity (BeepWidget target) ----
// The widget's button runs the same Talk to Beep intent as the Action Button: the app opens straight
// into live mode with the launch glow (from the middle, as for a control). It has no reply to show,
// since the widget cannot read the app's chats, so both sizes are just the button.
// While a live session runs, a Live Activity shows its state with a small orb in the Dynamic Island
// and on the Lock Screen; a tap goes back to the session, End leaves live mode, and it ends with it.
(() => {
  const st = document.createElement("style");
  st.textContent = `.tw-orb { border-radius: 50%; background: radial-gradient(circle at 36% 30%, #A6EBCF 0%, var(--m-ok) 46%, #0F3B2C 100%); flex: none; display: block }
.tw-orb.speak { background: radial-gradient(circle at 36% 30%, #FFD0C7 0%, var(--m-accent) 46%, #5A231C 100%) }
.tw-orb.think { background: radial-gradient(circle at 36% 30%, #7FBFA6 0%, #2E6F59 46%, #0D2A20 100%); animation: dbreath 2.4s ease-in-out infinite }
.tw-orb.listen { animation: twpulse 1.2s ease-in-out infinite }
@keyframes twpulse { 50% { transform: scale(1.12) } }
.island { position: absolute; left: 50%; transform: translateX(-50%); top: 11px; background: #000; color: #fff; z-index: 13; display: flex; align-items: center }
.island.compact { width: 200px; height: 36px; border-radius:var(--radius); justify-content: space-between; padding: 0 10px }
.island.expanded { width: 352px; border-radius: 40px; padding: 16px 18px 14px; flex-direction: column; align-items: stretch; gap: 10px; top: 8px }
.island .lw { font-size: 12px; font-weight: 600; letter-spacing: .04em }
.island .lw.on { color: #A6EBCF } .island .lw.sp { color: #FFB59E } .island .lw.th { color: #92A59A }
.island .bars { display: inline-flex; gap: 2px; align-items: center; height: 14px }
.island .bars i { width: 3px; border-radius: 2px; background: currentColor; animation: twbar .9s ease-in-out infinite }
.island .bars i:nth-child(2) { animation-delay: .15s } .island .bars i:nth-child(3) { animation-delay: .3s } .island .bars i:nth-child(4) { animation-delay: .45s }
@keyframes twbar { 0%, 100% { height: 4px } 50% { height: 14px } }
.island .xr { display: flex; align-items: center; gap: 12px }
.island .xt { flex: 1; min-width: 0; display: grid; gap: 2px }
.island .xt b { font-size: 15px; font-weight: 600 } .island .xt span { font-size: 13px; color: #B9C6BF; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical }
.island .end { border: 0; border-radius: 999px; padding: 8px 14px; background: var(--m-accent); color: var(--m-ink); font-weight: 600; font-size: 13px }
.lockact { position: absolute; left: 12px; right: 12px; bottom: 96px; border-radius:var(--radius); background: rgba(16,24,20,.72); backdrop-filter: blur(20px); color: #fff; padding: 14px 16px; display: flex; align-items: center; gap: 12px }
.lockact .xt { flex: 1; min-width: 0; display: grid; gap: 2px } .lockact b { font-size: 15px; font-weight: 600 } .lockact span { font-size: 13px; color: #C9D4CE }`;
  document.head.append(st);
})();
S.i.tw = { act: "compact", state: "listen" };
const TW_STATE = { listen: ["Listening", "on", "What’s on my calendar tomorrow…"], think: ["Thinking", "th", "Checking Google Calendar"], speak: ["Speaking", "sp", "You have the dentist at 9:30, then a call with Alex at 11."] };
Object.assign(P, {
  iTalkWidget: () => {
    const app = (c) => `<div style="display:grid;justify-items:center;gap:4px"><i style="width:56px;height:56px;border-radius:var(--radius);background:${c};display:block"></i><span style="width:40px;height:6px;border-radius:3px;background:rgba(255,255,255,.55)"></span></div>`;
    const btn = (inner, style) => `<button style="border:0;border-radius:var(--radius);background:var(--m-page);box-shadow:0 2px 10px rgba(16,32,26,.15);text-align:left;${style}" data-igo="iLiveLaunch" data-iset="lvFromControl:1" aria-label="Talk to Beep">${inner}</button>`;
    const pill = `<span style="display:inline-flex;align-items:center;gap:8px;padding:9px 14px 9px 9px;border-radius:999px;background:var(--m-accent);border:1.5px solid var(--m-ledge);box-shadow:2px 2px 0 var(--m-ledge);font-weight:600;font-size:14px;color:var(--m-ink)">${small(IC.wave, 16)}Talk</span>`;
    return `<div style="position:absolute;inset:0;background:linear-gradient(165deg,#9CC8B5,#E9C6B8);padding:46px 22px 0;display:flex;flex-direction:column;gap:22px">
      ${btn(`<span class="tw-orb" style="width:52px;height:52px"></span><span style="flex:1;min-width:0;display:grid;gap:2px"><span class="title" style="font-size:20px">Talk to Beep</span><span style="font-size:13px;color:var(--m-muted)">Live voice, hands free</span></span>${pill}`, "display:flex;align-items:center;gap:14px;padding:16px")}
      <div style="display:grid;grid-template-columns:repeat(4,1fr);gap:20px 0;justify-items:center">${["#F4B860", "#6FA8DC", "#E06666", "#93C47D"].map(app).join("")}</div>
      <div style="display:flex;gap:22px;align-items:flex-start">
        ${btn(`<span class="tw-orb" style="width:58px;height:58px"></span><span style="display:grid;gap:2px"><span class="title" style="font-size:19px">Talk to Beep</span><span style="font-size:12px;color:var(--m-muted)">Tap to talk</span></span>`, "width:150px;height:150px;flex:none;padding:14px;display:flex;flex-direction:column;justify-content:space-between")}
        <div style="display:grid;grid-template-columns:repeat(2,1fr);gap:20px 22px">${["#6D9EEB", "#F6B26B", "#A4C2F4", "#B6D7A8"].map(app).join("")}</div>
      </div>
    </div>`;
  },
  iLiveActivity: () => {
    const [word, cls, line] = TW_STATE[S.i.tw.state];
    const orbCls = S.i.tw.state;
    const bars = `<span class="bars lw ${cls}"><i></i><i></i><i></i><i></i></span>`;
    if (S.i.tw.act === "lock") return `<div class="icc" style="background:linear-gradient(170deg,#2F4F43,#0E1A15);display:block;padding:60px 0 0">
      <div style="text-align:center;color:#fff;display:grid;gap:2px"><span style="font-size:16px;font-weight:500">Thursday 30 October</span><span style="font-family:var(--display);font-weight:800;font-size:88px;line-height:1">9:41</span></div>
      <button class="lockact" style="border:0;text-align:left" data-igo="iLive"><span class="tw-orb ${orbCls}" style="width:40px;height:40px"></span><span class="xt"><b>Beep · ${word}</b><span>${line}</span></span>
        <span class="end" style="border-radius:999px;padding:8px 14px;background:var(--m-accent);color:var(--m-ink);font-weight:600;font-size:13px" data-igo="iLiveLeft">End</span></button>
    </div>`;
    const island = S.i.tw.act === "expanded"
      ? `<button class="island expanded" style="border:0;text-align:left" data-igo="iLive"><span class="xr"><span class="tw-orb ${orbCls}" style="width:44px;height:44px"></span><span class="xt"><b>Beep · ${word}</b><span>${line}</span></span></span>
          <span class="xr" style="justify-content:space-between">${bars}<span class="end" data-igo="iLiveLeft">End</span></span></button>`
      : `<button class="island compact" style="border:0" data-igo="iLive" aria-label="Back to live mode"><span class="tw-orb ${orbCls}" style="width:22px;height:22px"></span><span class="lw ${cls}">${word}</span></button>`;
    // The session runs on while the user is in another app: Safari stands in for it here.
    return `<div style="position:absolute;left:0;right:0;top:-50px;bottom:-22px;background:#fff;z-index:12;border-radius:46px;overflow:hidden;color:#1B1F1D">
      ${island}
      <div style="padding:64px 18px 0;display:grid;gap:12px"><div style="height:38px;border-radius:var(--radius);background:#EEF0EF;display:flex;align-items:center;padding:0 12px;font-size:14px;color:#6A706C">news.example</div>
        <div style="font-size:22px;font-weight:700;line-height:28px">Rain moves in on Saturday afternoon</div>
        ${[90, 100, 96, 70, 100, 84].map(w => `<span style="height:10px;border-radius:5px;background:#E6E9E7;width:${w}%;display:block"></span>`).join("")}</div>
    </div>`;
  },
});
const IOS_WIDGETS = IOS_SCREENS.find(g => g.group === "Widgets");
IOS_WIDGETS.items.push(["iTalkWidget", "Talk to Beep widget", "TalkToBeepWidget"]);
IOS_SCREENS.find(g => g.group === "Live voice").items.splice(5, 0, ["iLiveActivity", "Live Activity", "LiveVoiceActivity"]);
const twSet = (key, list) => list.map(([label, val]) => [label, () => { S.i.tw[key] = val; render(); }, () => S.i.tw[key] === val]);
VARIANTS.iLiveActivity = [...twSet("act", [["Compact", "compact"], ["Expanded", "expanded"], ["Lock Screen", "lock"]]), ...twSet("state", [["Listening", "listen"], ["Thinking", "think"], ["Speaking", "speak"]])];
// The widget opens live mode like a control: the glow starts at the middle of the top edge.
document.getElementById("ios").addEventListener("click", e => { if (e.target.closest('[data-iset="lvFromControl:1"]')) S.i.lv.from = "control"; }, true);

// Settings: a card on putting live voice on the Action Button, the controls, and when a pause sends.
S.i.lvPause = true;
// Live voice speed, one setting per device; both phones share it here.
const VOICE_SPEEDS = ["1×", "1.25×", "1.5×", "2×"];
S.voiceSpeed = "1.25×";
document.addEventListener("click", e => { const b = e.target.closest("[data-vspeed]"); if (b) { S.voiceSpeed = b.dataset.vspeed; render(); } });
const lvSettingsCard = () => `<div class="card" style="display:grid;gap:12px">
  <div class="row" style="align-items:center"><div class="logo" style="background:${TINTS[1]}">${IC.wave}</div>
    <div class="grow"><b style="font-weight:600">Talk to Beep</b><span class="muted" style="font-size:13px;line-height:19px">Live voice from anywhere, without opening a chat first.</span></div></div>
  <ol style="margin:0;padding-left:20px;display:grid;gap:4px;font-size:13px;line-height:19px">
    <li>Open <b style="font-weight:600">Settings</b>, then <b style="font-weight:600">Action Button</b>.</li>
    <li>Swipe to <b style="font-weight:600">Shortcut</b> and pick <b style="font-weight:600">Beep</b>, <b style="font-weight:600">Talk to Beep</b>.</li>
  </ol>
  <div class="muted" style="font-size:12px;line-height:18px">No Action Button? Use Back Tap: <b style="font-weight:600">Settings</b>, <b style="font-weight:600">Accessibility</b>, <b style="font-weight:600">Touch</b>, <b style="font-weight:600">Back Tap</b>, then <b style="font-weight:600">Double Tap</b> and <b style="font-weight:600">Talk to Beep</b>. Or add the Talk to Beep control to Control Center or the Lock Screen, or the widget to the Home Screen. In Japan you can also choose Beep for pressing and holding the side button.</div>
  <div style="display:flex;gap:8px;align-items:center"><button class="pill" style="min-height:36px;padding:0 14px;background:var(--m-user)" data-itoast="Opens Settings">Open Settings</button><button class="text-btn" style="color:var(--m-muted)" data-igo="iLiveLaunch">Try it</button></div>
  <div class="hr" style="margin:0"></div>
  <div style="display:flex;align-items:center;gap:12px"><div class="grow"><b style="font-weight:500;font-size:14px">Send when I stop talking</b><span class="muted" style="font-size:12px">${S.i.lvPause ? "A pause sends. Say “send” to send at once." : "Only “send” sends; pauses never do."}</span></div>
    <button class="switch" role="switch" aria-checked="${S.i.lvPause}" data-iact="lvPause" aria-label="Send when I stop talking"></button></div>
  <div class="hr" style="margin:0"></div>
  <div style="display:grid;gap:8px"><div><b style="font-weight:500;font-size:14px">Voice speed</b><span class="muted" style="font-size:12px;display:block">How fast Beep talks in live mode.</span></div>
    <div class="chips">${VOICE_SPEEDS.map(k => `<button class="fchip" aria-pressed="${S.voiceSpeed === k}" data-vspeed="${k}">${k}</button>`).join("")}</div></div>
</div>`;
const iSettingsBeforeLive = P.iSettings;
P.iSettings = () => { const h = iSettingsBeforeLive(), at = h.indexOf('<div class="card" style="display:grid;gap:12px">'); return at < 0 ? h : h.slice(0, at) + lvSettingsCard() + h.slice(at); };
P.iLiveSetup = () => { const h = P.iSettings(); return h.replace('<div class="body" style="padding:8px 20px 20px;gap:20px">', '<div class="body lvscroll" style="padding:8px 20px 20px;gap:20px">'); };
VARIANTS.iLiveSetup = [["Send on pause", () => { S.i.lvPause = true; render(); }, () => S.i.lvPause], ["Wait for “send”", () => { S.i.lvPause = false; render(); }, () => !S.i.lvPause]];
document.getElementById("ios").addEventListener("click", e => { if (e.target.closest('[data-iact="lvPause"]')) { S.i.lvPause = !S.i.lvPause; render(); } });
const renderBeforeLive = render;
render = function () {
  renderBeforeLive();
  ilvLoop();
  // The Action Button hint sits below the admin settings; scroll it into view.
  const card = S.screen === "iLiveSetup" && document.querySelector("#ios .lvscroll .card");
  if (card) card.scrollIntoView({ block: "start" });
};
S.toolLabels = S.toolLabels || "run";
S.grants = S.grants || "list";

// A routine with grants says so under its schedule and opens its own grants.
const routinesGrantBase = R.routines;
R.routines = () => {
  const link = (text, st) => `<button class="text-btn" style="align-self:flex-start;padding:0;min-height:28px;font-size:12px;display:flex;gap:4px;align-items:center" data-set="grants:${st}" data-go="grants">${small(I.shield, 14)}${text}</button>`;
  return routinesGrantBase()
    .replace("Weekdays at 13:30</span>", `Weekdays at 13:30</span>${link("1 grant, ends in 5 h", "one")}`)
    .replace("Every Sunday at 18:00</span>", `Every Sunday at 18:00</span>${link("1 grant, ends in 6 d", "list")}`);
};

try { const s = localStorage.getItem("beep-screens"); if (s && P[s]) S.screen = s; } catch (e) {}
const hash = location.hash.slice(1);
if (P[hash]) S.screen = hash;
render();
