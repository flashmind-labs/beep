import { flushSync } from "react-dom";
import { renderChat } from "../chat/chat.ts";
import { S } from "./core.ts";
import { type BarPageSlot, openBarPage } from "./bar-app.tsx";
import { loadModels } from "../settings/model.ts";
import { renderSettings } from "../settings/settings.ts";
import { invoke } from "../ui/tauri.ts";
import { paneShown } from "../ui/active.ts";
import type { LinkActions, LinkScreen } from "./link-view.tsx";

export interface LinkView { uri: string; code: string; qr_svg: string; expires_in: number }
export type PhoneInvite = Omit<LinkView, "uri">;
export type LinkState = "waiting" | "linked" | "expired";
export type LoginStep = "signin" | "browser" | "signfail" | "ownserver";
let offer: (LinkView & { until: number }) | null = null;
let login: LoginStep = "signin", loginError = "";
let screen: LinkScreen = { kind: "loading" }, generation = 0;
let view: BarPageSlot | null = null;

function mount() {
  return view ??= openBarPage(() => { view = null; generation++; clearInterval(S.timer); });
}
function draw(next: LinkScreen) {
  screen = next;
  const page = mount();
  flushSync(() => page.show({ view: "link", props: { screen, actions } }));
}
const actions: LinkActions = {
  again: () => void renderLink(), here: () => void useHere(), joinScreen: () => renderJoinCode(),
  loginStep: step => { generation++; login = step; loginError = ""; drawLogin(); }, browser: () => void openBrowser(), token: value => completeToken(value), connect: code => void connect(code), join: code => void join(code), back: () => void renderLink(),
  inviteAgain: () => void renderInvitePhone(), inviteBack: () => { generation++; clearInterval(S.timer); renderSettings(); }, startOver: () => void invoke("start_over"),
};

export async function renderLink() {
  mount();
  const mine = ++generation;
  S.view = "login"; clearInterval(S.timer); draw({ kind: "loading" });
  const hosted = await invoke<boolean>("login_hosted").catch(() => false);
  if (mine !== generation || S.view !== "login") return;
  if (hosted) return drawLogin();
  S.view = "link";
  try {
    const value = await invoke<LinkView>("link_offer");
    if (mine !== generation || S.view !== "link") return;
    offer = { ...value, until: Date.now() + value.expires_in * 1000 };
  } catch (error) {
    if (mine === generation && S.view === "link") draw({ kind: "error", title: "Link to your phone", message: String(error) });
    return;
  }
  drawLink("waiting");
  clearInterval(S.timer);
  S.timer = setInterval(() => {
    if (mine !== generation || S.view !== "link" || !offer) return clearInterval(S.timer);
    const left = Math.max(0, Math.round((offer.until - Date.now()) / 1000));
    if (!left) { clearInterval(S.timer); drawLink("expired"); }
    // A hidden bar shows no seconds; the next tick after it shows draws them.
    else if (screen.kind === "link" && screen.state === "waiting" && paneShown()) draw({ ...screen, seconds: left });
  }, 1000);
}
export function drawLink(state: LinkState) {
  if (!offer) return;
  draw({ kind: "link", state, offer, seconds: Math.max(0, Math.round((offer.until - Date.now()) / 1000)), busy: false, error: "" });
}
function drawLogin(busy = false) { S.view = "login"; draw({ kind: "login", step: login, error: loginError, busy }); }
async function openBrowser() {
  if (screen.kind !== "login" || screen.busy) return;
  const mine = ++generation; drawLogin(true);
  try { await invoke("start_login"); if (mine === generation && S.view === "login") { login = "browser"; loginError = ""; drawLogin(); } }
  catch (error) { if (mine === generation && S.view === "login") loginFailed(error); }
}
function completeToken(value: string) {
  const token = value.trim();
  if (!/^[\w-]+\.[\w-]+$/.test(token) || screen.kind !== "login" || screen.busy) return;
  const mine = ++generation; drawLogin(true);
  void invoke("complete_login", { token }).catch(error => { if (mine === generation && S.view === "login") loginFailed(error); });
}
async function connect(code: string) {
  if (screen.kind !== "login" || screen.busy) return;
  const mine = ++generation; drawLogin(true);
  try { await invoke("use_own_server", { code }); }
  catch (error) { if (mine === generation && S.view === "login") { loginError = String(error); drawLogin(); } }
}
export function loginFailed(error: unknown) {
  const e = String(error); loginError = e.startsWith("bad request:") ? "That sign-in expired or was for another device." : e;
  login = "signfail";
  if (S.view === "login") drawLogin();
}
async function useHere() {
  if (screen.kind !== "link" || screen.busy) return;
  const previous = screen, mine = ++generation; clearInterval(S.timer); S.view = "starting"; draw({ ...previous, busy: true });
  try { await invoke("use_here"); if (mine !== generation || S.view !== "starting") return; S.link = "linked"; loadModels(); renderSettings(); }
  catch (error) { if (mine === generation && S.view === "starting") { S.view = "link"; draw({ ...previous, busy: false, error: String(error) }); } }
}
function renderJoinCode(error = "") { generation++; clearInterval(S.timer); S.view = "joincode"; draw({ kind: "join", error, busy: false }); }
async function join(code: string) {
  if (screen.kind !== "join" || screen.busy) return;
  const mine = ++generation; S.view = "joining"; draw({ ...screen, busy: true });
  try {
    const name = await invoke<string>("join_account", { code });
    if (mine !== generation || S.view !== "joining") return;
    S.link = "linked"; draw({ kind: "joined", name });
    setTimeout(() => { if (mine === generation && S.view === "joining") renderChat(); }, 1800);
  } catch (error) { if (mine === generation && S.view === "joining") renderJoinCode(/not found/i.test(String(error)) ? "That code is used up or expired. Ask for a new one." : String(error)); }
}
export async function renderInvitePhone() {
  mount();
  const mine = ++generation; S.view = "invite-phone"; clearInterval(S.timer);
  try {
    const value = await invoke<PhoneInvite>("invite_phone");
    if (mine !== generation || S.view !== "invite-phone") return;
    const until = Date.now() + value.expires_in * 1000;
    draw({ kind: "invite", invite: value, expired: false });
    // The invite shows no countdown, so one wake at its expiry is enough.
    S.timer = setTimeout(() => {
      if (mine === generation && S.view === "invite-phone") draw({ kind: "invite", invite: value, expired: true });
    }, until - Date.now());
  } catch (error) { if (mine === generation && S.view === "invite-phone") draw({ kind: "error", title: "Link a phone", message: String(error) }); }
}
export function renderRevoked() { generation++; clearInterval(S.timer); S.view = "revoked"; draw({ kind: "revoked" }); }
