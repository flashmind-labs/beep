import { agentMessage } from "./session-frame.tsx";
import { Spinner } from "../ui/kit.tsx";
import { useLayoutEffect, useRef } from "react";
import { mentionText, shortSize } from "../agentpane/files-state.ts";
import { paneSession } from "../agentpane/media.ts";
import { openViewer } from "../chat/media.ts";
import { Icon } from "../ui/icons.tsx";
import { AG } from "./agents.ts";
import { drawAgentAttached } from "./images.ts";
import { answerUpload, MN, removeMention } from "./workdrop.ts";

const keys = new WeakMap<object, number>();
let next = 0;
function key(value: object) {
  let id = keys.get(value);
  if (id === undefined) { id = ++next; keys.set(value, id); }
  return id;
}

export function AgentAttachments() {
  const ask = MN.ask, there = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { if (ask) queueMicrotask(() => { if (MN.ask === ask) there.current?.focus(); }); }, [ask]);
  const machine = paneSession()?.machine_name ?? "the machine";
  return <>{AG.att.map((image, i) => <span className="att" key={key(image)}><button type="button" className="preview" data-att-preview={i} aria-label={`Preview ${image.name}`} title="Expand image" onClick={event => openViewer(AG.att.map(a => ({ url: a.url, name: a.name, saves: true })), i, event.currentTarget.closest<HTMLElement>(".cc-main") ?? undefined)}><img src={image.url} alt="" /><span className="nm">{image.name}</span></button><button type="button" aria-label={`Remove ${image.name}`} data-agatt={i} onClick={() => { AG.att.splice(i, 1); drawAgentAttached(); agentMessage.current?.focus(); }}><Icon name="x" /></button></span>)}
    {MN.paths.map((path, i) => <span className="agment" key={path} title={`Goes in the message as ${mentionText([path])}`}><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M14 3H6v18h12V7z" /><path d="M14 3v4h4" /></svg><span className="nm">{path}</span><button aria-label={`Remove ${path}`} data-agment={i} onClick={() => removeMention(i)}><Icon name="x" /></button></span>)}
    {MN.uploads.map(upload => <span className="agment up" key={key(upload)} title={`Uploading to ${machine}`}><Spinner /><span className="nm">{upload.name}</span><small>{upload.size ? Math.round(upload.done / upload.size * 100) : 0}%</small></span>)}
    {ask && <div className="agupask"><span>Upload {ask.files.length === 1 ? <b>{ask.files[0].name}</b> : `${ask.files.length} files`} ({shortSize(ask.files.reduce((n, f) => n + f.size, 0))}) to {machine}:</span><button ref={there} className="dbtn pri" data-agup="there" onClick={() => answerUpload("there")}>Into {ask.folder ? `${ask.folder}/` : "the session's folder"}</button><button className="dbtn" data-agup="default" title="Outside the project. Deleted when this session is closed." onClick={() => answerUpload("default")}>Temporary attachments</button><button className="dbtn quiet" data-agup="cancel" onClick={() => answerUpload("cancel")}>Cancel</button></div>}
    {AG.attError && <span className="derr">{AG.attError}</span>}</>;
}
