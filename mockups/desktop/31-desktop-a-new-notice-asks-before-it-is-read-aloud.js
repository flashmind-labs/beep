// ---- Desktop: a new notice asks before it is read aloud ----
// With Voice notes on "Ask first" (the default), a new notice never starts playing by itself.
// While you are at this Mac, not on a call and the bar is closed, the orb comes up with a small
// card: who or what it is from, two lines of the text, Play, Open and a close button. Play reads
// it aloud as a voice note did (the orb speaks, Option tap or the orb's close button stops it).
// Open, a click on the card, or Option twice opens the bar on that chat. Untouched for 20
// seconds (hovering holds it), the card goes and the usual macOS notification shows instead.
// On a call or away, only the notification shows. "Read aloud right away" keeps the old
// behaviour; "Off" only notifies.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .dorbnote { position: absolute; top: 106px; right: 12px; z-index: 5; width: 256px; border-radius: 12px; overflow: hidden; cursor: pointer;
  background: rgba(21,22,23,.92); backdrop-filter: blur(20px); border: 1px solid var(--d-border); box-shadow: 0 10px 28px rgba(0,0,0,.5); animation: barIn .22s var(--ease) }
.mac .dorbnote:hover { border-color: #34363C }
.mac .dorbnote .head { display: flex; align-items: center; gap: 10px; padding: 9px 8px 0 11px }
.mac .dorbnote .head .tt { flex: 1; min-width: 0; display: grid; line-height: 17px }
.mac .dorbnote .head b { font-size: 13px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .dorbnote .head small { font-size: 11.5px; color: var(--d-muted) }
.mac .dorbnote .head .x { width: 22px; height: 22px; border-radius: 50%; border: 0; background: transparent; color: var(--d-muted); display: grid; place-items: center; padding: 0; flex: none; align-self: flex-start }
.mac .dorbnote .head .x:hover { background: var(--d-elev); color: var(--d-fg) }
.mac .dorbnote .snip { padding: 6px 10px 0; font-size: 12.5px; line-height: 17px; color: rgba(227,228,230,.85); display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden }
.mac .dorbnote .acts { display: flex; gap: 6px; padding: 10px 12px 10px }
.mac .dorbnote .acts .dbtn { flex: 1; justify-content: center; min-height: 26px; padding: 0 8px; font-size: 12px }
.mac .dorbnote .life { height: 2px; background: var(--d-border) }
.mac .dorbnote .life i { display: block; height: 100%; background: var(--d-emerald); animation: life 20s linear forwards }
.mac .dorbnote:hover .life i, .mac .dorbnote.hov .life i { animation-play-state: paused }
.mac .dorbnote.hov { border-color: #34363C }`;
  document.head.append(st);
})();

/// The card under the orb for a new notice.
const dOrbNote = hover => `<div class="dorbnote${hover ? " hov" : ""}" role="button" aria-label="Open the chat" data-dgo="dSpokenChat">
    <div class="head"><div class="tt"><b>${VN_TITLE}</b><small>New message</small></div>
      <button class="x" aria-label="Dismiss" data-dact="dset:nc:none">${MINI_X}</button></div>
    <div class="snip">${VN_LINES.join(" ")}</div>
    <div class="acts"><button class="dbtn pri" data-dact="dset:nc:play"><svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>Play</button><button class="dbtn" data-dgo="dSpokenChat">Open<kbd>⌥ ⌥</kbd></button></div>
    <div class="life"><i></i></div>
  </div>`;
const dNoteBanner = `<div class="nbanner" role="alert"><div class="main" data-dgo="dSpokenChat"><span class="ic">${DI.tray}</span>
      <div class="tx"><div class="top"><b>Beep</b><small>now</small></div><b>${VN_TITLE}</b><span>${VN_LINES.join(" ")}</span></div></div></div>`;
D.dNoticeCard = () => {
  const v = S.d.nc || "card";
  if (v === "loading") return deskScene(dMini("think", false));
  if (v === "play") return deskScene(dMini("speak", false, { xLabel: "Stop the voice note", xAct: 'data-dact="dset:nc:none"' }));
  if (v === "gone") return deskScene(dNoteBanner, { badge: true });
  if (v === "none") return deskScene("");
  return deskScene(dMini("idle", false, { xLabel: "Dismiss", xAct: 'data-dact="dset:nc:none"' }) + dOrbNote(v === "hover"));
};
VARIANTS.dNoticeCard = dv("nc", [["Arrives", "card"], ["Hover holds it", "hover"], ["Play, loading", "loading"], ["Play", "play"], ["After 20 s", "gone"], ["Dismissed", "none"]]);
{
  const g = DESK_SCREENS.find(g => g.group === "Live voice").items;
  g.splice(g.findIndex(it => it[0] === "dLiveAsk") + 1, 0, ["dNoticeCard", "A new notice", "OrbWindow, notice"]);
}
{
  const st = document.createElement("style");
  st.textContent = `.mac .dorb { --c2: var(--d-emerald) }
.mac .dorb.speak { --c2: var(--d-coral) }
.mac .dorb.think { --c2: var(--d-emerald) }
.mac .dorb.idle { --c2: var(--d-muted) }
.mac .dorb .core { background: var(--c2); box-shadow: none; transform: scale(calc(1 + var(--lvl) * .22)) }
.mac .dorb .ring { border: 1.5px solid color-mix(in srgb, var(--c2) 30%, transparent) }
.mac .dorb .ring.b { border-color: color-mix(in srgb, var(--c2) 15%, transparent) }
.mac .dmini { background: color-mix(in srgb, var(--d-surface) 86%, transparent); backdrop-filter: none; box-shadow: 0 4px 14px rgba(0,0,0,.35); animation: none }
.mac .dmini.hold { box-shadow: 0 0 0 3px color-mix(in srgb, var(--d-emerald) 55%, transparent), 0 4px 14px rgba(0,0,0,.35) }
.mac .oring { position: absolute; inset: -4px; width: 72px; height: 72px; pointer-events: none; transform: rotate(-90deg) }
.mac .oring circle { fill: none; stroke: var(--d-emerald); stroke-width: 3; stroke-linecap: round; stroke-dasharray: 207.4; stroke-dashoffset: 62 }
.mac .dorbtoast { position: absolute; top: 100px; right: 12px; z-index: 5; max-width: 200px; padding: 4px 10px; border-radius: 999px; background: color-mix(in srgb, var(--d-surface) 92%, transparent); border: 1px solid var(--d-border); font-size: 12px; font-weight: 500; line-height: 16px; color: var(--d-fg); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; box-shadow: 0 6px 14px rgba(0,0,0,.4) }
.mac .dorbtoast.bad { color: var(--d-err) }
.mac .dorbask, .mac .dorbnote { background: color-mix(in srgb, var(--d-surface) 94%, transparent); backdrop-filter: none; box-shadow: 0 8px 20px rgba(0,0,0,.45); animation: none }
.mac .dorbask .err { padding: 8px 12px 0; font-size: 12px; color: var(--d-err) }
.mac .dorbnote .snip { color: var(--d-fg); overflow-wrap: anywhere }`;
  document.head.append(st);
}
// The "A voice note arrives" variant is now what Read aloud right away does.
VARIANTS.dSpeak = dv("ls", [["Reply", "reply"], ["Read aloud right away", "note"]]);

// Settings: Voice notes is a choice of three, Ask first by default.
const VN_CHOICES = [["ask", "Ask first"], ["aloud", "Read aloud right away"], ["off", "Off"]];
const VN_SAYS = {
  ask: "Shown by the orb with Play when you are here and not on a call",
  aloud: "Read aloud when you are here and not on a call",
  off: "Notification only",
};
const dVoiceNotes = () => {
  const cur = S.d.vn || "ask";
  return `<div class="dtoggle"><span><b>Voice notes</b><small>${VN_SAYS[cur]}</small></span></div>
        <div class="dchips" role="radiogroup" aria-label="Voice notes" style="margin-top:-6px">${VN_CHOICES.map(([k, label]) => `<button class="dchip" role="radio" aria-pressed="${cur === k}" aria-checked="${cur === k}" data-dact="dset:vn:${k}">${label}</button>`).join("")}</div>`;
};
for (const id of ["dSettings", "dLiveSettings"]) {
  const base = D[id];
  D[id] = (...a) => base(...a).replace(/<div class="dtoggle"><span><b>Voice notes<\/b>[\s\S]*?<\/button><\/div>/, dVoiceNotes());
}
VARIANTS.dLiveSettings = dv("vn", VN_CHOICES.map(([k, label]) => [label, k]));
DESK_SCREENS.find(g => g.group === "Live voice").items.forEach(it => { if (it[0] === "dLiveSettings") it[1] = "Voice settings"; });

// After drawing: a live chat shows its newest turn, and a phone's dock orb moves.
{
  const base = render;
  render = function () {
    base();
    const root = document.getElementById("desk");
    if (!root.querySelector(".ldock, .ddock")) return;
    root.querySelectorAll(".body, .dbody").forEach(b => { b.scrollTop = b.scrollHeight; });
  };
}
