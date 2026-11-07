// ---- Desktop: the orb asks ----
// Live mode in the orb, and the reply needs an OK (an acting tool, or a routine's grant).
// A small card hangs under the orb: the action as a question, the app, up to three fields,
// and Allow, Always, Deny (a grant has no Always). The orb window grows to fit it, anchored
// to the top right, and shrinks back once it is answered here or in the bar. Clicking the
// card, not a button, opens the bar on the chat with the whole approval card. No keys and
// no spoken answers: the orb never takes focus.
// Any approval that comes while live mode is in the orb shows here, not only the live reply's:
// background work and other chats ask the same way, with where it came from under the action.
// Live mode says "I need your OK to send a message in Slack." once, when nothing else plays.
// The app's real logo sits in the tile (the Apps page's logo_url); a letter only when the app
// has none. Allow for this session sits under Allow and Deny. An approval still waiting when
// live mode ends shows as its notification, and its card stays in the chat.
(() => {
  const st = document.createElement("style");
  st.textContent = `.mac .dorbask { position: absolute; top: 106px; right: 12px; z-index: 5; width: 256px; border-radius: 12px; overflow: hidden; cursor: pointer;
  background: rgba(21,22,23,.92); backdrop-filter: blur(20px); border: 1px solid var(--d-border); box-shadow: 0 10px 28px rgba(0,0,0,.5); animation: barIn .22s var(--ease) }
.mac .dorbask:hover { border-color: #34363C }
.mac .dorbask .head { display: flex; align-items: center; gap: 10px; padding: 8px 12px; background: rgba(242,153,74,.1); border-bottom: 1px solid var(--d-border) }
.mac .dorbask .head .tt { flex: 1; min-width: 0; display: grid; line-height: 17px }
.mac .dorbask .head b { font-size: 13px; font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .dorbask .head small { font-size: 11.5px; color: var(--d-muted) }
.mac .dorbask .dapp { width: 24px; height: 24px; font-size: 11px }
.mac .dorbask .kv { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 2px 10px; padding: 8px 12px 0; font-size: 12px; line-height: 17px }
.mac .dorbask .kv span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap }
.mac .dorbask .kv span:nth-child(odd) { color: var(--d-muted) }
.mac .dorbask .more { padding: 3px 11px 0; font-size: 11.5px; color: var(--d-dim) }
.mac .dorbask .acts { display: flex; gap: 6px; padding: 10px 12px 11px }
.mac .dorbask .acts .dbtn { flex: 1; justify-content: center; min-height: 26px; padding: 0 8px; font-size: 12px }
.mac .dorbask .acts { flex-wrap: wrap }
.mac .dorbask .acts .dbtn.wide { flex-basis: 100% }
.mac .dorbask .dapp svg { width: 100%; height: 100%; display: block }`;
  document.head.append(st);
})();

/// The card under the orb. `grant` drops Always, like the bar's card for a routine.
function dOrbAsk(o) {
  const shown = o.fields.slice(0, 3), more = o.fields.length - shown.length;
  return `<div class="dorbask" role="button" aria-label="Open the chat to see the whole request" data-dact="dset:approve:${o.grant ? "grant" : "ask"}" data-dgo="dApprove">
    <div class="head"><span class="dapp">${o.logo || o.app[0]}</span><div class="tt"><b>${o.action}?</b><small>${o.from || `${o.app} asks first`}</small></div></div>
    ${shown.length ? `<div class="kv">${shown.map(([k, v]) => `<span>${k}</span><span>${v}</span>`).join("")}</div>` : ""}
    ${more > 0 ? `<div class="more">${more} more in the chat</div>` : ""}
    ${o.behind ? `<div class="more">${o.behind} more waiting after this</div>` : ""}
    ${o.err ? `<div class="err">${o.err}</div>` : ""}
    <div class="acts"><button class="dbtn pri" data-dact="dset:lq:done" data-toast="${o.grant ? "Allowed for this routine" : "Allowed once"}">Allow</button>${o.grant ? "" : `<button class="dbtn" data-dact="dset:lq:done" data-toast="Allowed, and always allowed from now on">Always</button>`}<button class="dbtn warn" data-dact="dset:lq:done" data-toast="Denied">Deny</button>${o.grant ? "" : `<button class="dbtn wide" data-dact="dset:lq:done" data-toast="Allowed for the rest of this chat">Allow for this session</button>`}</div>
  </div>`;
}
const SLACK_LOGO = `<svg viewBox="0 0 24 24" aria-hidden="true"><rect width="24" height="24" rx="6" fill="#fff"/><g transform="translate(4 4) scale(.6667)"><path fill="#E01E5A" d="M5 15a2 2 0 1 1-2-2h2zm1 0a2 2 0 1 1 4 0v5a2 2 0 1 1-4 0z"/><path fill="#36C5F0" d="M9 5a2 2 0 1 1 2-2v2zm0 1a2 2 0 1 1 0 4H4a2 2 0 1 1 0-4z"/><path fill="#2EB67D" d="M19 9a2 2 0 1 1 2 2h-2zm-1 0a2 2 0 1 1-4 0V4a2 2 0 1 1 4 0z"/><path fill="#ECB22E" d="M15 19a2 2 0 1 1-2 2v-2zm0-1a2 2 0 1 1 0-4h5a2 2 0 1 1 0 4z"/></g></svg>`;
D.dLiveAsk = () => {
  const v = S.d.lq || "ask";
  if (v === "done") return deskScene(dMini("think", false));
  const card = v === "grant"
    ? dOrbAsk({ grant: true, app: "Gmail", action: "Reply to thread for a routine", fields: [["Routine", "Inbox sweep"], ["Allows", "1 reply each time it runs, without asking"], ["To", "ana@example.com only"], ["Ends", "When the routine ends, or in 8 h"]] })
    : v === "work" ? dOrbAsk({ app: "Slack", logo: SLACK_LOGO, from: "Background work asks first", action: "Send message", fields: [["Channel", "#launch"], ["Text", "Review moved to Friday 10:00"]] })
    : dOrbAsk({ app: "Fastmail", action: "Send email", behind: v === "more" ? 2 : 0, err: v === "err" ? "The server did not answer" : "", fields: [["To", "Alex Chen"], ["Subject", "Plumber on Thursday, 9 to 12 works for me"], ["Body", "Hi Alex, Thursday morning is fine. The door code is the same as last time."], ...(v === "more" ? [["Cc", "sam@example.com"]] : [])] });
  return deskScene(dMini("listen", false) + card);
};
VARIANTS.dLiveAsk = dv("lq", [["Send email", "ask"], ["More behind it", "more"], ["Could not answer", "err"], ["Background work, Slack", "work"], ["For a routine", "grant"], ["Answered", "done"]]);
{
  const g = DESK_SCREENS.find(g => g.group === "Live voice").items;
  g.splice(g.findIndex(it => it[0] === "dLiveMini") + 1, 0, ["dLiveAsk", "The orb asks", "OrbWindow, approval"]);
}
