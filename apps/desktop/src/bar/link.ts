import { renderChat } from "../chat/chat.ts";
import { copy } from "../chat/chats.ts";
import { $, $$, bar, esc, ICON, invoke, S } from "./core.ts";
import { loadModels } from "../settings/model.ts";
import { renderSettings } from "../settings/settings.ts";

// ---------- Link ----------

/// A code for the phone to scan (LinkView).
interface LinkView {
  uri: string;
  code: string;
  qr_svg: string;
  /// Seconds.
  expires_in: number;
}

/// A code another device scans or types to join this Mac's account (PhoneInvite).
interface PhoneInvite {
  code: string;
  qr_svg: string;
  /// Seconds.
  expires_in: number;
}

/// What the link view shows: waiting for the phone, linked, or the code ran out.
export type LinkState = "waiting" | "linked" | "expired";

/// The link offer on screen, with when it runs out in milliseconds.
let offer: (LinkView & { until: number }) | null = null;

export async function renderLink() {
  S.view = "link";
  clearInterval(S.timer);
  try {
    const view = await invoke<LinkView>("link_offer");
    offer = Object.assign(view, { until: Date.now() + view.expires_in * 1000 });
  } catch (e) {
    bar.innerHTML = `<div class="dwin"><h4>Link to your phone</h4><p class="derr">${esc(e)}</p></div>`;
    return;
  }
  // Another view, or a newer code, took over while this one loaded.
  if (S.view !== "link") return;
  clearInterval(S.timer);
  drawLink("waiting");
  S.timer = setInterval(() => {
    if (S.view !== "link") return clearInterval(S.timer);
    const left = Math.max(0, Math.round((offer!.until - Date.now()) / 1000));
    const el = $("#count");
    if (el) el.textContent = `${Math.floor(left / 60)}:${String(left % 60).padStart(2, "0")}`;
    if (!left) { clearInterval(S.timer); drawLink("expired"); }
  }, 1000);
}

export function drawLink(state: LinkState) {
  const o = offer!;
  const over = state === "linked"
    ? `<div class="over"><span style="display:grid;justify-items:center;gap:6px"><span style="width:44px;height:44px;border-radius:50%;background:var(--accent);color:#fff;display:grid;place-items:center">${ICON.check}</span>Linked</span></div>`
    : state === "expired" ? '<div class="over">Code expired</div>' : "";
  const qr = `<div class="qr${state === "waiting" ? "" : " dim"}">${o.qr_svg}${over}</div>`;
  const parts = {
    waiting: `<h4>Link to your phone</h4><p>Scan with Beep on your phone: Settings, Linked devices.</p>${qr}<div class="code">${esc(o.code)}</div>
      <div class="wait"><span class="dspin"></span>Waiting for phone…</div><p style="font-size:12px">Code expires in <span id="count" style="font-variant-numeric:tabular-nums">10:00</span></p>`,
    linked: `<h4>Linked</h4><p>This computer now sees your chats and memories.</p>${qr}`,
    expired: `<h4>Link to your phone</h4><p>Scan with Beep on your phone: Settings, Linked devices.</p>${qr}<button class="dbtn pri" id="again">New code</button><p style="font-size:12px">A code lasts ten minutes.</p>`,
  };
  const alone = state === "linked" ? "" : `<div class="dor"><span>or</span></div>
    <button class="dbtn pri" id="here">Use Beep on this Mac</button>
    <p style="font-size:12px">Starts an account on this Mac. You can link your phone to it later.</p>
    <button class="dbtn quiet" id="havecode">Have a join code?</button>`;
  bar.innerHTML = `<div class="dwin">${parts[state]}${alone}</div>`;
  $("#again")?.addEventListener("click", renderLink);
  $("#here")?.addEventListener("click", useHere);
  $("#havecode")?.addEventListener("click", () => renderJoinCode());
}

async function useHere(e: Event) {
  const button = e.currentTarget as HTMLButtonElement;
  button.disabled = true;
  clearInterval(S.timer);
  // Not "link", so the link-state event does not show "Linked to your phone".
  S.view = "starting";
  try {
    await invoke("use_here");
    S.link = "linked";
    loadModels();
    renderSettings();
  } catch (err) {
    S.view = "link";
    button.disabled = false;
    button.insertAdjacentHTML("afterend", `<p class="derr">${esc(err)}</p>`);
  }
}

/// Typing the code another computer shows, from the link view or Settings.
function renderJoinCode(error = "", back: () => unknown = renderLink) {
  clearInterval(S.timer);
  S.view = "joincode";
  bar.innerHTML = `<div class="dwin"><h4>Join an account</h4>
    <label class="dfield${error ? " err" : ""}" style="width:100%;text-align:left">Join code<input id="jcode" class="mono" maxlength="20" autocomplete="off" spellcheck="false" placeholder="K7QM4-X9TRP">
      <span class="note">${esc(error || "The code your other computer shows under Settings, Devices.")}</span></label>
    <p style="font-size:12px">This Mac's chats and memories move to that account.</p>
    <div class="row" style="display:flex;gap:8px"><button class="dbtn" id="jback">Back</button><button class="dbtn pri" id="jjoin">Join<kbd>${ICON.ret}</kbd></button></div></div>`;
  $$("#jback").addEventListener("click", back);
  const input = $$<HTMLInputElement>("#jcode");
  const go = async () => {
    const code = input.value;
    $$<HTMLButtonElement>("#jjoin").disabled = true;
    S.view = "joining";
    try {
      const name = await invoke<string>("join_account", { code });
      S.link = "linked";
      bar.innerHTML = `<div class="dwin"><h4>Joined</h4><p>This Mac is now on ${esc(name)}'s account.</p></div>`;
      setTimeout(renderChat, 1800);
    } catch (err) {
      renderJoinCode(/not found/i.test(String(err)) ? "That code is used up or expired. Ask for a new one." : String(err), back);
    }
  };
  $$("#jjoin").addEventListener("click", go);
  input.addEventListener("keydown", e => { if (e.key === "Enter") go(); });
  input.focus();
}

/// A code for a phone to join this Mac's account with.
export async function renderInvitePhone() {
  S.view = "invite-phone";
  clearInterval(S.timer);
  let inv: PhoneInvite;
  try {
    inv = await invoke<PhoneInvite>("invite_phone");
  } catch (e) {
    bar.innerHTML = `<div class="dwin"><h4>Link a phone</h4><p class="derr">${esc(e)}</p></div>`;
    return;
  }
  if (S.view !== "invite-phone") return;
  const until = Date.now() + inv.expires_in * 1000;
  bar.innerHTML = `<div class="dwin"><h4>Link a phone</h4>
    <p>Scan with Beep on your phone: Settings, Linked devices. The phone's chats and memories move to this Mac's account.</p>
    <div class="qr" id="iqr">${inv.qr_svg}</div><div class="code" style="font-size:22px;font-weight:600">${esc(inv.code)}</div>
    <p style="font-size:12px" id="ileft">A code lasts ten minutes.</p>
    <div style="display:flex;gap:8px"><button class="dbtn" id="iback">Back</button><button class="dbtn pri" id="icopy">Copy code</button></div></div>`;
  $$("#icopy").addEventListener("click", e => copy(inv.code, e.currentTarget as HTMLElement));
  $$("#iback").addEventListener("click", () => { clearInterval(S.timer); renderSettings(); });
  S.timer = setInterval(() => {
    if (S.view !== "invite-phone") return clearInterval(S.timer);
    if (Date.now() < until) return;
    clearInterval(S.timer);
    $("#iqr")?.classList.add("dim");
    $$("#ileft").innerHTML = 'Code expired. <button class="dbtn quiet" id="inew">New code</button>';
    $$("#inew").addEventListener("click", renderInvitePhone);
  }, 1000);
}

export function renderRevoked() {
  S.view = "revoked";
  bar.innerHTML = `<div class="dwin"><h4>This Mac was removed</h4>
    <p>Your phone took this computer off the account. To use Beep here again, start over with a new key and link it from the phone.</p>
    <button class="dbtn pri" id="over">Start over</button></div>`;
  $$("#over").addEventListener("click", () => invoke("start_over"));
}
