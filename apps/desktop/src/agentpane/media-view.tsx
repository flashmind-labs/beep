import { Spinner } from "../ui/kit.tsx";
import { useLayoutEffect, useRef, useState } from "react";
import { ModelControls } from "./model-controls.tsx";
import { fileSize } from "../chat/media.ts";
import { invoke } from "../ui/tauri.ts";
import { baseName, resolvePath } from "./media-state.ts";
import { P, type MediaTab } from "./media.ts";
import { readError, readFile } from "./read.ts";
import { noteTab } from "./media-draw.ts";

export function FileActions({ tab, machine, label }: { tab: MediaTab; machine: string | null; label: string }) {
  if (!tab.path) return null;
  const path = tab.path;
  const note = (text: string) => noteTab(tab, text);
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

/// A video tab. Its element is the tab's, so the pane pauses it out of
/// sight; moved to the other dock, it goes on from where it was.
export function VideoView({ tab, machine }: { tab: MediaTab; machine: string | null }) {
  const [meta, setMeta] = useState(fileSize(tab.size));
  const [failed, setFailed] = useState(false);
  const video = (node: HTMLVideoElement | null) => {
    if (node) { tab.video = node; return; }
    if (tab.video) tab.videoAt = tab.video.currentTime;
    tab.video = null;
  };
  return <>{failed ? <div className="agpmsg">This video does not play here. Open it in QuickTime instead.</div> : <div className="agpstage"><video ref={video} controls playsInline preload="auto" src={tab.url ?? undefined} onLoadedMetadata={event => {
    const node = event.currentTarget, seconds = Math.round(node.duration);
    const time = Number.isFinite(seconds) ? `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")} · ` : "";
    setMeta(`${time}${node.videoWidth}×${node.videoHeight} · ${fileSize(tab.size)}`);
    if (tab.videoAt) { node.currentTime = tab.videoAt; tab.videoAt = 0; }
  }} onError={() => setFailed(true)} onEnded={event => event.currentTarget.load()} /></div>}
  <div className="agpacts"><span className="meta">{tab.note ?? meta}</span><span className="sp" /><FileActions tab={tab} machine={machine} label="Open in QuickTime" /></div></>;
}

export function PdfView({ tab, machine }: { tab: MediaTab; machine: string | null }) {
  return <><div className="agpstage pdf"><iframe title={tab.name} src={tab.url ?? undefined} /></div><div className="agpacts"><span className="meta">{tab.note ?? `PDF · ${fileSize(tab.size)}`}</span><span className="sp" /><FileActions tab={tab} machine={machine} label="Open in Preview" /></div></>;
}

function ModelFooter({ tab, machine }: { tab: MediaTab; machine: string | null }) {
  const [wire, setWire] = useState(!!tab.wire);
  const model = tab.model, info = model?.info;
  const clips = info?.clips ? ` · ${info.clips} clip${info.clips > 1 ? "s" : ""}` : "";
  const meta = info ? `${info.format} · ${info.triangles.toLocaleString()} triangles${info.meshes > 1 ? ` · ${info.meshes} meshes` : ""}${clips} · ${fileSize(tab.size)}` : "";
  return <><button className="dbtn" data-reset="1" disabled={!model} onClick={() => model?.reset()}>Reset view</button><button className={`dbtn${wire ? " on" : ""}`} data-wire="1" disabled={!model} onClick={() => {
    const next = !wire;
    tab.wire = next; setWire(next); model?.wireframe(next);
  }}>Wireframe</button><span className="meta">{tab.note ?? meta}</span><span className="sp" /><span className="hint">Drag to orbit, right-drag to pan, wheel to zoom</span><FileActions tab={tab} machine={machine} label="Open" /></>;
}

/// A tab still reading, or one that failed.
export function PendingView({ tab }: { tab: MediaTab }) {
  const machine = P.session?.machine_name ?? null;
  const pending = tab.error ? { message: tab.error, error: true, progress: null }
    : tab.kind === "text" ? { message: `Reading ${tab.name} from ${machine ?? "the machine"}…`, error: false, progress: null }
    : tab.kind === "term" ? { message: `Opening a terminal on ${(tab.at ?? tab.existing)?.machine_name ?? "the machine"}…`, error: false, progress: null }
    : { message: `Opening ${tab.name}…`, error: false, progress: tab.size ? Math.min(1, Math.max(0, tab.done / tab.size)) : 0 };
  return <div className={`agpmsg${pending.error ? " derr" : ""}`}>{!pending.error && <Spinner />}{pending.message}{pending.progress !== null && <span className="agpload"><i style={{ transform: `scaleX(${pending.progress})` }} /></span>}</div>;
}

type ModelState = { kind: "drawing" } | { kind: "drawn" } | { kind: "failed"; message: string };

/// A 3D model tab. Three.js draws its canvas into the stage; React owns the
/// stage's messages, the animation controls and the footer. The model is the
/// tab's: moved to the other dock, its canvas moves into the new stage.
export function ModelView({ tab, bytes, machine }: { tab: MediaTab; bytes: ArrayBuffer; machine: string | null }) {
  const stage = useRef<HTMLDivElement>(null);
  const [state, setState] = useState<ModelState>(tab.model ? { kind: "drawn" } : { kind: "drawing" });

  useLayoutEffect(() => {
    const box = stage.current;
    if (!box) return;
    if (tab.model) { tab.model.place(box); return; }
    let live = true;
    void (async () => {
      try {
        const { mountModel } = await import("./model3d.ts");
        const view = await mountModel(box, bytes, tab.name.slice(tab.name.lastIndexOf(".") + 1).toLowerCase(), sibling(tab));
        if (!live || !P.tabs.includes(tab)) return view.dispose();
        tab.model = view;
        setState({ kind: "drawn" });
        view.resize();
      } catch (e) {
        if (live && P.tabs.includes(tab)) setState({ kind: "failed", message: e instanceof Error ? e.message : String(e) });
      }
    })();
    return () => { live = false; };
  }, [tab, bytes]);

  const animation = state.kind === "drawn" ? tab.model?.anim ?? null : null;
  return <><div className="agpstage model" ref={stage}>{state.kind === "drawing" ? <div className="agpmsg"><Spinner />{`Drawing ${tab.name}…`}</div>
    : state.kind === "failed" ? <div className="agpmsg derr">{`Could not draw ${tab.name}: ${state.message}`}</div> : null}</div>
    <div className="agpacts anim" hidden={!animation}>{animation && <ModelControls animation={animation} />}</div><div className="agpacts model-actions"><ModelFooter tab={tab} machine={machine} /></div></>;
}

/// A .gltf's buffers and textures are files beside it.
function sibling(tab: MediaTab) {
  const dir = tab.path ? tab.path.slice(0, tab.path.lastIndexOf("/") + 1) : "";
  return async (uri: string) => {
    if (!P.session || !dir || /^[a-z]+:/i.test(uri)) return null;

    try {
      return (await readFile(P.session.machine_id, resolvePath(dir, decodeURIComponent(uri)), () => {})).bytes;
    } catch {
      return null;
    }
  };
}
