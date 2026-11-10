import { clock } from "../agents/agentturns.ts";
import { FRAME_SANDBOX, KIND_NAMES, relativeTo, type Artifact } from "./artifacts-state.ts";

export function ArtifactsList({ artifacts, cwd, picked, gone, when }: { artifacts: readonly Artifact[]; cwd: string; picked: string | null; gone: ReadonlySet<string>; when: (artifact: Artifact) => number | undefined }) {
  if (!artifacts.length) return <div className="agpmsg">Pages the agent publishes, writes outside its folder or makes with a command show here. Repo files it edits are in Files.</div>;
  return <>{artifacts.map(artifact => {
    const removed = gone.has(artifact.path), at = when(artifact), meta = [artifact.turnNo ? `Turn ${artifact.turnNo}` : "", at != null ? clock(at) : ""].filter(Boolean).join(" · ");
    return <button type="button" key={artifact.path} className={`agart${artifact.path === picked ? " on" : ""}${removed ? " gone" : ""}`} role="option" aria-selected={artifact.path === picked} data-apath={artifact.path} title={artifact.path}>
      <span className={`k ${artifact.kind}`}>{KIND_NAMES[artifact.kind]}</span><span className="t"><b>{artifact.name}</b><small>{relativeTo(cwd, artifact.path) ?? artifact.path}</small></span><span className="r">{artifact.url && <span className="pub">claude.ai</span>}<span>{removed ? "Removed" : meta}</span>{!removed && (artifact.versions > 1 ? <small>{artifact.versions} versions</small> : artifact.made ? <small>Made by a command</small> : null)}</span></button>;
  })}</>;
}

export function ArtifactActions({ folded, note, url, inside }: { folded: boolean; note: string; url: string | null; inside: boolean }) {
  return <><button type="button" className="dbtn" data-aact="fold" title="Fold or show the preview" aria-expanded={!folded}>{folded ? "Show" : "Fold"}</button><span className="meta mono">{note}</span><span className="sp" />
    <button type="button" className="dbtn" data-aact="claude" hidden={!url} title={url ?? ""}>Open on claude.ai</button><button type="button" className="dbtn" data-aact="file" disabled={!inside} title={inside ? "Show it in the Files tab" : "It is outside the session's folder"}>Open file</button><button type="button" className="dbtn" data-aact="copy">Copy path</button></>;
}

export type ArtifactPreview =
  | { kind: "message"; text: string; error: boolean }
  | { kind: "loading"; name: string; machine: string }
  | { kind: "pdf"; name: string }
  | { kind: "html"; name: string; src: string }
  | { kind: "md"; html: string }
  | { kind: "image"; name: string; mime: string; data: string };

export function ArtifactPreviewView({ value }: { value: ArtifactPreview }) {
  switch (value.kind) {
    case "message": return <div className={`agpmsg${value.error ? " derr" : ""}`}>{value.text}</div>;
    case "loading": return <div className="agpmsg"><span className="dspin" />Reading {value.name} from {value.machine}…</div>;
    case "pdf": return <div className="agpmsg">PDFs open in a tab of their own. <button className="dbtn" data-aact="pdf">Open {value.name}</button></div>;
    case "html": return <div className="agartframe-box"><iframe className="agartframe" sandbox={FRAME_SANDBOX} referrerPolicy="no-referrer" title={value.name} src={value.src} /></div>;
    case "md": return <div className="agartmd" dangerouslySetInnerHTML={{ __html: value.html }} />;
    case "image": return <div className="agartimg"><img alt={value.name} src={`data:${value.mime};base64,${value.data}`} /></div>;
  }
}
