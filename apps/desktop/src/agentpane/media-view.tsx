import { useState } from "react";
import { fileSize } from "../chat/media.ts";
import { invoke } from "../ui/tauri.ts";
import { baseName } from "./media-state.ts";
import type { MediaTab } from "./media.ts";
import { readError } from "./read.ts";

function FileActions({ tab, machine, label, note }: { tab: MediaTab; machine: string | null; label: string; note(text: string): void }) {
  if (!tab.path) return null;
  const path = tab.path;
  const open = () => { if (machine) void invoke("agent_file_open", { machineId: machine, path }).catch(error => note(readError(error))); };
  const save = () => {
    if (!machine) return;
    void invoke<string>("agent_file_save", { machineId: machine, path }).then(to => {
      note(`Saved to Downloads as ${baseName(to)}`);
      void invoke("reveal_file", { path: to }).catch(() => {});
    }).catch(error => note(readError(error)));
  };
  return <><button className="dbtn" data-mopen="1" onClick={open}>{label}</button><button className="dbtn" data-msave="1" onClick={save}>Save</button></>;
}

export function VideoView({ tab, machine }: { tab: MediaTab; machine: string | null }) {
  const [meta, setMeta] = useState(fileSize(tab.size));
  const [failed, setFailed] = useState(false);
  return <>{failed ? <div className="agpmsg">This video does not play here. Open it in QuickTime instead.</div> : <div className="agpstage"><video controls playsInline preload="auto" src={tab.url ?? undefined} onLoadedMetadata={event => {
    const video = event.currentTarget, seconds = Math.round(video.duration);
    const time = Number.isFinite(seconds) ? `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")} · ` : "";
    setMeta(`${time}${video.videoWidth}×${video.videoHeight} · ${fileSize(tab.size)}`);
  }} onError={() => setFailed(true)} onEnded={event => event.currentTarget.load()} /></div>}
  <div className="agpacts"><span className="meta">{meta}</span><span className="sp" /><FileActions tab={tab} machine={machine} label="Open in QuickTime" note={setMeta} /></div></>;
}

export function PdfView({ tab, machine }: { tab: MediaTab; machine: string | null }) {
  const [meta, setMeta] = useState(`PDF · ${fileSize(tab.size)}`);
  return <><div className="agpstage pdf"><iframe title={tab.name} src={tab.url ?? undefined} /></div><div className="agpacts"><span className="meta">{meta}</span><span className="sp" /><FileActions tab={tab} machine={machine} label="Open in Preview" note={setMeta} /></div></>;
}
