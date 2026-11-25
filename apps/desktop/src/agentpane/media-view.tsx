import { Button, Spinner } from "../ui/kit.tsx";
import { useCallback, useLayoutEffect, useRef, useState, type SyntheticEvent } from "react";
import { ModelControls } from "./model-controls.tsx";
import { fileSize } from "../chat/media.tsx";
import { revealLabel } from "../ui/platform.ts";
import { invoke } from "../ui/tauri.ts";
import { resolvePath } from "./media-state.ts";
import { transferSpeed } from "./files-state.ts";
import { P, saveFile, type MediaTab, openTabs } from "./media.ts";
import { readError, readFile } from "./read-file.ts";
import { noteTab } from "./media-draw.ts";
import { PdfExportView } from "./pdf-export-view.tsx";

export function FileActions({ tab, machine, label }: { tab: MediaTab; machine: string | null; label: string }) {
  const saved = tab.exported;
  if (saved) return <Button onClick={() => void invoke("reveal_file", { path: saved.path }).catch(error => noteTab(tab, readError(error)))}>{revealLabel() === "Share" ? "Share" : "Show in folder"}</Button>;
  if (!tab.path) return null;
  const path = tab.path;
  const note = (text: string) => noteTab(tab, text);
  const session = P.session?.id ?? null;
  const open = () => { if (machine) void invoke("agent_file_open", { machineId: machine, path, session }).catch(error => note(readError(error))); };
  const save = () => {
    if (!machine) return;
    saveFile("agent_file_save", { machineId: machine, path, session }, note);
  };
  return <><Button data-mopen="1" onClick={open}>{label}</Button><Button data-msave="1" onClick={save}>Save</Button></>;
}

/// A video or audio tab. Its element is the tab's, so the pane pauses it out
/// of sight; moved to the other dock, it goes on from where it was.
export function PlayerView({ tab, machine, video }: { tab: MediaTab; machine: string | null; video: boolean }) {
  const [meta, setMeta] = useState(fileSize(tab.size));
  const [failed, setFailed] = useState(false);
  const player = useCallback((node: HTMLMediaElement | null) => keepPlayer(tab, node), [tab]);
  const loaded = (event: SyntheticEvent<HTMLMediaElement>) => {
    const node = event.currentTarget;
    const size = node instanceof HTMLVideoElement ? `${node.videoWidth}×${node.videoHeight} · ` : "";
    setMeta(`${duration(node.duration)}${size}${fileSize(tab.size)}`);
    resume(tab, node);
  };
  return <>{failed
    ? <div className="agpmsg">{video ? "This video does not play here. Open it in QuickTime instead." : "This audio does not play here. Open it in another app instead."}</div>
    : video
      ? <div className="agpstage"><video ref={player} controls playsInline preload="auto" src={tab.url ?? undefined} onLoadedMetadata={loaded} onError={() => setFailed(true)} onEnded={event => event.currentTarget.load()} /></div>
      : <div className="agpstage audio"><audio ref={player} controls preload="auto" src={tab.url ?? undefined} onLoadedMetadata={loaded} onError={() => setFailed(true)} /></div>}
  <div className="agpacts"><span className="meta">{tab.note ?? meta}</span><span className="sp" /><FileActions tab={tab} machine={machine} label={video ? "Open in QuickTime" : "Open"} /></div></>;
}

/// Keeps a tab's player, and where it stood when its view went.
function keepPlayer(tab: MediaTab, node: HTMLMediaElement | null) {
  if (node) { tab.player = node; return; }
  if (tab.player) tab.playerAt = tab.player.currentTime;
  tab.player = null;
}

function resume(tab: MediaTab, node: HTMLMediaElement) {
  if (tab.playerAt) { node.currentTime = tab.playerAt; tab.playerAt = 0; }
}

/// `m:ss · ` for a length in seconds, or "" when it is not known.
function duration(length: number) {
  const seconds = Math.round(length);
  return Number.isFinite(seconds) ? `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")} · ` : "";
}

export function PdfView({ tab, machine }: { tab: MediaTab; machine: string | null }) {
  if (tab.exported) return <PdfExportView blob={tab.exported.blob} name={tab.name} note={tab.note}>
    <FileActions tab={tab} machine={machine} label="Open in Preview" />
  </PdfExportView>;
  return <><div className="agpstage pdf"><iframe title={tab.name} src={tab.url ?? undefined} /></div><div className="agpacts"><span className="meta">{tab.note ?? `PDF · ${fileSize(tab.size)}`}</span><span className="sp" /><FileActions tab={tab} machine={machine} label="Open in Preview" /></div></>;
}

function ModelFooter({ tab, machine }: { tab: MediaTab; machine: string | null }) {
  const [wire, setWire] = useState(!!tab.wire);
  const model = tab.model, info = model?.info;
  const clips = info?.clips ? ` · ${info.clips} clip${info.clips > 1 ? "s" : ""}` : "";
  const meta = info ? `${info.format} · ${info.triangles.toLocaleString()} triangles${info.meshes > 1 ? ` · ${info.meshes} meshes` : ""}${clips} · ${fileSize(tab.size)}` : "";
  return <><Button data-reset="1" disabled={!model} onClick={() => model?.reset()}>Reset view</Button><Button className={wire ? "on" : ""} data-wire="1" disabled={!model} onClick={() => {
    const next = !wire;
    tab.wire = next; setWire(next); model?.wireframe(next);
  }}>Wireframe</Button><span className="meta">{tab.note ?? meta}</span><span className="sp" /><span className="hint">Drag to orbit, right-drag to pan, wheel to zoom</span><FileActions tab={tab} machine={machine} label="Open" /></>;
}

/// A tab still reading, or one that failed.
export function PendingView({ tab }: { tab: MediaTab }) {
  const machine = P.session?.machine_name ?? null;
  const speed = tab.kind === "text" || tab.kind === "term" || tab.error ? null : transferSpeed(tab);
  const pending = tab.error ? { message: tab.error, error: true, progress: null }
    : tab.kind === "text" ? { message: `Reading ${tab.name} from ${machine ?? "the machine"}…`, error: false, progress: null }
    : tab.kind === "term" ? { message: `Opening a terminal on ${(tab.at ?? tab.existing)?.machine_name ?? "the machine"}…`, error: false, progress: null }
    : { message: `Opening ${tab.name}…`, error: false, progress: tab.size ? Math.min(1, Math.max(0, tab.done / tab.size)) : 0 };
  return <div className={`agpmsg${pending.error ? " derr" : ""}`}>{!pending.error && <Spinner />}{pending.message}{pending.progress !== null && <span className="agpload"><i style={{ transform: `scaleX(${pending.progress})` }} /></span>}{speed && <small>{speed}</small>}</div>;
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
        if (!live || !openTabs().includes(tab)) return view.dispose();
        tab.model = view;
        setState({ kind: "drawn" });
        view.resize();
      } catch (e) {
        if (live && openTabs().includes(tab)) setState({ kind: "failed", message: e instanceof Error ? e.message : String(e) });
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
      return (await readFile(P.session.machine_id, resolvePath(dir, decodeURIComponent(uri)), P.session.id, () => {})).bytes;
    } catch {
      return null;
    }
  };
}
