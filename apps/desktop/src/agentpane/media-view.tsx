import { useState, type RefCallback } from "react";
import { ModelControls } from "./model-controls.tsx";
import { setSessionPortal } from "../agents/session-frame.tsx";
import { fileSize } from "../chat/media.ts";
import { invoke } from "../ui/tauri.ts";
import { baseName } from "./media-state.ts";
import type { MediaTab } from "./media.ts";
import { readError } from "./read.ts";

export function FileActions({ tab, machine, label, note }: { tab: MediaTab; machine: string | null; label: string; note(text: string): void }) {
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

export function ModelFooter({ tab, machine }: { tab: MediaTab; machine: string | null }) {
  const [wire, setWire] = useState(!!tab.wire);
  const [note, setNote] = useState<string | null>(null);
  const model = tab.model, info = model?.info;
  const clips = info?.clips ? ` · ${info.clips} clip${info.clips > 1 ? "s" : ""}` : "";
  const meta = info ? `${info.format} · ${info.triangles.toLocaleString()} triangles${info.meshes > 1 ? ` · ${info.meshes} meshes` : ""}${clips} · ${fileSize(tab.size)}` : "";
  return <><button className="dbtn" data-reset="1" disabled={!model} onClick={() => model?.reset()}>Reset view</button><button className={`dbtn${wire ? " on" : ""}`} data-wire="1" disabled={!model} onClick={() => {
    const next = !wire;
    tab.wire = next; setWire(next); model?.wireframe(next);
  }}>Wireframe</button><span className="meta">{note ?? meta}</span><span className="sp" /><span className="hint">Drag to orbit, right-drag to pan, wheel to zoom</span><FileActions tab={tab} machine={machine} label="Open" note={setNote} /></>;
}

interface PendingState { message: string; error: boolean; progress: number | null }
const pending = new WeakMap<HTMLElement, PendingState>();

/// Loading progress updates only the pane that is reading the file.
export function drawPending(host: HTMLElement, tab: MediaTab, machineName: string | null) {
  const next: PendingState = tab.error ? { message: tab.error, error: true, progress: null }
    : tab.kind === "text" ? { message: `Reading ${tab.name} from ${machineName ?? "the machine"}…`, error: false, progress: null }
    : tab.kind === "term" ? { message: `Opening a terminal on ${(tab.at ?? tab.existing)?.machine_name ?? "the machine"}…`, error: false, progress: null }
    : { message: `Opening ${tab.name}…`, error: false, progress: tab.size ? Math.min(1, Math.max(0, tab.done / tab.size)) : 0 };
  const before = pending.get(host);
  if (before && before.message === next.message && before.error === next.error && before.progress === next.progress) return;
  pending.set(host, next);
  setSessionPortal(host, <div className={`agpmsg${next.error ? " derr" : ""}`}>{!next.error && <span className="dspin" />}{next.message}{next.progress !== null && <span className="agpload"><i style={{ transform: `scaleX(${next.progress})` }} /></span>}</div>);
}

export type ModelState = { kind: "drawing" } | { kind: "drawn" } | { kind: "failed"; message: string };

/// A 3D model tab. Three.js draws its canvas into the stage; React owns the
/// stage's messages, the animation controls and the footer.
export function ModelView({ tab, machine, state, stage }: { tab: MediaTab; machine: string | null; state: ModelState; stage: RefCallback<HTMLDivElement> }) {
  const animation = state.kind === "drawn" ? tab.model?.anim ?? null : null;
  return <><div className="agpstage model" ref={stage}>{state.kind === "drawing" ? <div className="agpmsg"><span className="dspin" />{`Drawing ${tab.name}…`}</div>
    : state.kind === "failed" ? <div className="agpmsg derr">{`Could not draw ${tab.name}: ${state.message}`}</div> : null}</div>
    <div className="agpacts anim" hidden={!animation}>{animation && <ModelControls animation={animation} />}</div><div className="agpacts model-actions"><ModelFooter tab={tab} machine={machine} /></div></>;
}
