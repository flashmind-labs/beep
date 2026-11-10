import { clock } from "../agents/agentturns.ts";
import { FRAME_SANDBOX, KIND_NAMES, relativeTo, type Artifact } from "./artifacts-state.ts";

export type ArtifactAction = "fold" | "pdf" | "claude" | "file" | "copy";

interface ArtifactsModel {
  artifacts: readonly Artifact[]; cwd: string; picked: string | null; gone: ReadonlySet<string>; when: (artifact: Artifact) => number | undefined;
  actions: { shown: boolean; folded: boolean; note: string; url: string | null; inside: boolean };
  preview: { key: number; value: ArtifactPreview } | null;
  /// Out of sight a page's frame is emptied so its scripts stop.
  live: boolean;
}

/// The Artifacts tab: the list, the picked one's preview and its actions.
export function ArtifactsPane({ model, pick, act }: { model: ArtifactsModel; pick(path: string): void; act(action: ArtifactAction): void }) {
  const { actions, preview } = model;
  return <><div className="agartlist" role="listbox" aria-label="Artifacts"><ArtifactsList artifacts={model.artifacts} cwd={model.cwd} picked={model.picked} gone={model.gone} when={model.when} pick={pick} /></div>
    <div className="agartshow" hidden={actions.folded}>{preview && <ArtifactPreviewView key={preview.key} value={preview.value} live={model.live} act={act} />}</div>
    <div className="agpacts" hidden={!actions.shown}><ArtifactActions folded={actions.folded} note={actions.note} url={actions.url} inside={actions.inside} act={act} /></div></>;
}

function ArtifactsList({ artifacts, cwd, picked, gone, when, pick }: { artifacts: readonly Artifact[]; cwd: string; picked: string | null; gone: ReadonlySet<string>; when: (artifact: Artifact) => number | undefined; pick(path: string): void }) {
  if (!artifacts.length) return <div className="agpmsg">Pages the agent publishes, writes outside its folder or makes with a command show here. Repo files it edits are in Files.</div>;
  return <>{artifacts.map(artifact => {
    const removed = gone.has(artifact.path), at = when(artifact), meta = [artifact.turnNo ? `Turn ${artifact.turnNo}` : "", at != null ? clock(at) : ""].filter(Boolean).join(" · ");
    return <button type="button" key={artifact.path} className={`agart${artifact.path === picked ? " on" : ""}${removed ? " gone" : ""}`} role="option" aria-selected={artifact.path === picked} title={artifact.path} onClick={() => pick(artifact.path)}>
      <span className={`k ${artifact.kind}`}>{KIND_NAMES[artifact.kind]}</span><span className="t"><b>{artifact.name}</b><small>{relativeTo(cwd, artifact.path) ?? artifact.path}</small></span><span className="r">{artifact.url && <span className="pub">claude.ai</span>}<span>{removed ? "Removed" : meta}</span>{!removed && (artifact.versions > 1 ? <small>{artifact.versions} versions</small> : artifact.made ? <small>Made by a command</small> : null)}</span></button>;
  })}</>;
}

function ArtifactActions({ folded, note, url, inside, act }: { folded: boolean; note: string; url: string | null; inside: boolean; act(action: ArtifactAction): void }) {
  return <><button type="button" className="dbtn" onClick={() => act("fold")} title="Fold or show the preview" aria-expanded={!folded}>{folded ? "Show" : "Fold"}</button><span className="meta mono">{note}</span><span className="sp" />
    <button type="button" className="dbtn" onClick={() => act("claude")} hidden={!url} title={url ?? ""}>Open on claude.ai</button><button type="button" className="dbtn" onClick={() => act("file")} disabled={!inside} title={inside ? "Show it in the Files tab" : "It is outside the session's folder"}>Open file</button><button type="button" className="dbtn" onClick={() => act("copy")}>Copy path</button></>;
}

export type ArtifactPreview =
  | { kind: "message"; text: string; error: boolean }
  | { kind: "loading"; name: string; machine: string }
  | { kind: "pdf"; name: string }
  | { kind: "html"; name: string; src: string }
  | { kind: "md"; html: string }
  | { kind: "image"; name: string; mime: string; data: string };

function ArtifactPreviewView({ value, live, act }: { value: ArtifactPreview; live: boolean; act(action: ArtifactAction): void }) {
  switch (value.kind) {
    case "message": return <div className={`agpmsg${value.error ? " derr" : ""}`}>{value.text}</div>;
    case "loading": return <div className="agpmsg"><span className="dspin" />Reading {value.name} from {value.machine}…</div>;
    case "pdf": return <div className="agpmsg">PDFs open in a tab of their own. <button className="dbtn" onClick={() => act("pdf")}>Open {value.name}</button></div>;
    case "html": return <div className="agartframe-box"><iframe className="agartframe" sandbox={FRAME_SANDBOX} referrerPolicy="no-referrer" title={value.name} src={live ? value.src : "about:blank"} /></div>;
    case "md": return <div className="agartmd" dangerouslySetInnerHTML={{ __html: value.html }} />;
    case "image": return <div className="agartimg"><img alt={value.name} src={`data:${value.mime};base64,${value.data}`} /></div>;
  }
}
