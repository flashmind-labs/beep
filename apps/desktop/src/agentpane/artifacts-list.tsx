import { clock } from "../agents/agentturns.ts";
import { KIND_NAMES, relativeTo, type Artifact } from "./artifacts-state.ts";

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
