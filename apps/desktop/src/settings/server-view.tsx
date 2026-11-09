import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { ICON } from "../ui/icons.ts";
import { invoke } from "../ui/tauri.ts";
import { ACTION_LABEL, SIGN_OUT_ASK, serverKind, serverView, type Reach } from "./server-state.ts";
import { signInWay } from "./subscription.ts";
import type { Device, MacSettings } from "./settings.ts";
import type { LinkState } from "../bar/core.ts";

const pending = new Set<string>(), listeners = new Set<() => void>();
let revision = 0;
function changed() { revision++; for (const listener of listeners) listener(); }
const subscribe = (listener: () => void) => { listeners.add(listener); return () => listeners.delete(listener); };
async function run(key: string, action: () => Promise<void>) {
  if (pending.has(key)) return;
  pending.add(key); changed();
  try { await action(); } finally { pending.delete(key); changed(); }
}
interface Props { settings: MacSettings; devices: Device[] | null; link: LinkState; signedOut: boolean; reach: Reach | null; refresh(): void; invite(): void; away(): void }
export function ServerView(p: Props) {
  useSyncExternalStore(subscribe, () => revision);
  const [confirm, setConfirm] = useState<"signout" | "unlink" | null>(null), [error, setError] = useState(""), [unlinkError, setUnlinkError] = useState("");
  const [tokenMode, setTokenMode] = useState(false), [token, setToken] = useState(""), [tokenError, setTokenError] = useState(""), [started, setStarted] = useState(false);
  const active = useRef(true), tokenInput = useRef<HTMLInputElement>(null);
  const kind = serverKind(p.link, p.signedOut, p.reach), view = serverView(kind);
  const kindRef = useRef(kind);
  kindRef.current = kind;
  useEffect(() => () => { active.current = false; }, []);
  useEffect(() => { if (tokenMode) tokenInput.current?.focus(); }, [tokenMode]);
  useEffect(() => {
    setStarted(false);
    if (kind !== "no-account") { setTokenMode(false); setToken(""); setTokenError(""); }
  }, [kind]);
  const safe = (f: () => void) => { if (active.current) f(); };
  const signIn = () => void run("sign-in", async () => {
    safe(() => setError(""));
    try { await invoke("sign_in"); safe(() => setStarted(true)); }
    catch (e) { safe(() => setError(String(e))); }
  });
  const subscribeAccount = () => void run("subscribe", async () => {
    safe(() => { setError(""); setTokenError(""); });
    try {
      if (signInWay(await invoke<boolean>("login_hosted")) === "here") { await invoke("use_here"); return; }
      await invoke("start_login");
      safe(() => { if (kindRef.current === "no-account") setTokenMode(true); });
    } catch (e) { safe(() => { if (kindRef.current === "no-account") setTokenError(String(e)); }); }
  });
  const complete = (value: string) => {
    setToken(value);
    const text = value.trim();
    if (!/^[\w-]+\.[\w-]+$/.test(text)) return;
    void run("token", async () => {
      try { await invoke("complete_login", { token: text }); }
      catch (e) { safe(() => { if (kindRef.current === "no-account") setTokenError(String(e)); }); }
    });
  };
  const signOut = () => void run("sign-out", async () => { try { await invoke("sign_out"); safe(() => { setConfirm(null); p.refresh(); }); } catch (e) { safe(() => setError(String(e))); } });
  const unlink = () => void run("unlink", async () => { try { await invoke("unlink"); safe(p.away); } catch (e) { safe(() => setUnlinkError(String(e))); } });
  const remove = (id: string) => void run(`remove:${id}`, async () => { try { await invoke("remove_device", { id }); } catch (e) { console.warn(e); } safe(p.refresh); });
  const asking = confirm === "signout" && view.actions.includes("sign-out"), devices = p.devices;
  const owner = devices?.some(device => device.this_device && device.primary) ?? false;
  return <><div className="dtoggle"><span><b>{view.title}</b><small>{view.note}</small></span>{!asking && view.actions.map(action => {
    const id = action === "sign-in" ? kind === "no-account" ? "subscribe" : "signIn" : action === "sign-out" ? "signOut" : "serverRetry";
    return <button className={`dbtn${action === "sign-in" ? " pri" : ""}`} id={id} key={action} disabled={action === "sign-in" && (started || pending.has(kind === "no-account" ? "subscribe" : "sign-in"))} onClick={action === "sign-in" ? kind === "no-account" ? subscribeAccount : signIn : action === "sign-out" ? () => { setError(""); setConfirm("signout"); } : p.refresh}>{ACTION_LABEL[action]}</button>;
  })}</div>
    {asking && <div className="dcard"><div className="dask"><b className="dtitle">{SIGN_OUT_ASK.title}</b><div className="dmuted dblock">{SIGN_OUT_ASK.note}</div><div className="dline"><button className="dbtn warn" id="signOutYes" disabled={pending.has("sign-out")} onClick={signOut}>{SIGN_OUT_ASK.confirm}</button><button className="dbtn quiet" id="signOutNo" onClick={() => { setError(""); setConfirm(null); }}>{SIGN_OUT_ASK.cancel}</button></div></div></div>}
    <div id="subNote">{tokenMode && <label className="dfield">Finish in your browser, or paste the token<input ref={tokenInput} id="subToken" className="mono" autoComplete="off" spellCheck={false} placeholder="Token" value={token} disabled={pending.has("token")} onChange={e => complete(e.currentTarget.value)} /></label>}{tokenError && <div className="derr" id="subErr">{tokenError}</div>}</div><div className="derr" id="serverErr">{error}</div>
    {view.devices && (devices === null ? <div className="dskel-rows"><div className="dskel" style={{ width: "78%" }} /><div className="dskel" style={{ width: "56%" }} /></div> : <><div className="dsec flat dgroup">Devices{(owner || confirm !== "unlink") && <button className="dbtn" id="invite" onClick={p.invite}>Link a phone…</button>}</div><div className="dcard ddevices">{devices.map(device => <div className="dtoggle" key={device.id}><span><b className="dline tight">{device.name}{device.this_device && <span className="dtag">This computer</span>}{device.primary && <span className="dtag">Owner</span>}</b><small>{device.kind === "phone" ? "Phone" : "Computer"}</small></span>{owner && !device.this_device && <button className="dbtn quiet" data-remove={device.id} aria-label={`Remove ${device.name}`} disabled={pending.has(`remove:${device.id}`)} onClick={() => remove(device.id)} dangerouslySetInnerHTML={{ __html: ICON.x }} />}</div>)}{!owner && confirm !== "unlink" && <div className="dtoggle"><span><small>This computer: {p.settings.name}</small></span><button className="dbtn warn" id="unlink" onClick={() => { setUnlinkError(""); setConfirm("unlink"); }}>Unlink this computer</button></div>}</div>{confirm === "unlink" && <div className="dcard"><div className="dask"><b className="dtitle">Unlink this computer?</b><div className="dmuted dblock">This computer stops seeing your chats and memories. Your phone keeps them. To use Beep here again, start over and link it from the phone.</div><div className="dline"><button className="dbtn warn" id="unlinkYes" disabled={pending.has("unlink")} onClick={unlink}>Unlink</button><button className="dbtn quiet" id="unlinkNo" onClick={() => { setUnlinkError(""); setConfirm(null); }}>Cancel</button></div><div className="derr" id="unlinkErr">{unlinkError}</div></div></div>}</>)}
  </>;
}
