import { useLayoutEffect, useRef } from "react";
import { ICON } from "../ui/icons.ts";
import { invoke } from "../ui/tauri.ts";
import type { LiveView } from "./live.ts";
import { liveParts, LV } from "./live.ts";

export function LiveDock({ view }: { view: LiveView }) {
  const parts = liveParts(view), caption = useRef<HTMLDivElement>(null), progress = useRef<HTMLElement>(null);
  const left = LV.soon ? Math.max(0, LV.soon.until - Date.now()) : 0;
  useLayoutEffect(() => { if (caption.current) caption.current.scrollTop = caption.current.scrollHeight; }, [parts.cap]);
  useLayoutEffect(() => {
    if (!progress.current || !LV.soon) return;
    progress.current.style.setProperty("--from", (left / LV.soon.total).toFixed(3));
    progress.current.style.animationDuration = `${left}ms`;
  }, [left, LV.soon]);
  return <div className="ddock" id="ldock" role="region" aria-label="Live mode">
    {LV.toast && <div className="ltoast" role="status">{LV.toast}</div>}
    <div className="drow1"><button className={`dorb ${parts.orb}`} id="orb" aria-label={parts.label} onClick={() => { void invoke("live_tap").catch(() => {}); }}><span className="ring b" /><span className="ring" /><span className="core" /></button>
      <div className="dtx"><div className={`word ${parts.word[1]}`}>{view.title && <><span className="tt">{view.title}</span> · </>}{parts.word[0]}</div><div className="cap" aria-live="polite" ref={caption} dangerouslySetInnerHTML={{ __html: parts.cap }} /></div>
      <button className="dleave" id="lleave" aria-label={parts.replying ? "Cancel the reply and leave live mode" : "Leave live mode"} onClick={() => { void invoke("cancel_live").catch(() => {}); }} dangerouslySetInnerHTML={{ __html: ICON.x }} /></div>
    <div className="dfoot">{LV.soon && parts.mode === "listen" && <div className="dsoon" role="progressbar" aria-label="Sending after the pause"><i ref={progress} /></div>}<div className="hint">{parts.hint}</div>{parts.replying && <button className="dbtn quiet lcancel" id="lcancel" onClick={() => { void invoke("cancel_live").catch(() => {}); }}>Cancel<kbd>esc</kbd></button>}</div>
  </div>;
}
