import { agentMessage } from "./session-frame.tsx";
import { Button, Spinner } from "../ui/kit.tsx";
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

/// Takes image `i` off the message, with the file it is a preview of.
function removeImage(i: number) {
  const [image] = AG.att.splice(i, 1);
  if (typeof image.original === "string") removeMention(MN.paths.indexOf(image.original));
  drawAgentAttached();
  agentMessage.current?.focus();
}

/// Takes the last chip off the message.
export function removeLastAttachment() {
  const loose = MN.paths.findLastIndex(path => !AG.att.some(image => image.original === path));
  if (loose >= 0) removeMention(loose);
  else if (AG.att.length) removeImage(AG.att.length - 1);
}

export function AgentAttachments() {
  const ask = MN.ask, there = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { if (ask) queueMicrotask(() => { if (MN.ask === ask) there.current?.focus(); }); }, [ask]);
  const machine = paneSession()?.machine_name ?? "the machine";
  return <>{AG.att.map((image, i) => <span className="att" key={key(image)}><button type="button" className="preview" data-att-preview={i} aria-label={`Preview ${image.name}`} title="Expand image" onClick={event => openViewer(AG.att.map(a => ({ url: a.url, name: a.name, saves: true })), i, event.currentTarget.closest<HTMLElement>(".cc-main") ?? undefined)}><img src={image.url} alt="" /><span className="nm">{image.name}</span></button><button type="button" aria-label={`Remove ${image.name}`} data-agatt={i} onClick={() => removeImage(i)}><Icon name="x" /></button></span>)}
    {MN.paths.map((path, i) => !AG.att.some(image => image.original === path) && <span className="agment" key={path} title={`Goes in the message as ${mentionText([path])}`}><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round"><path d="M14 3H6v18h12V7z" /><path d="M14 3v4h4" /></svg><span className="nm">{path}</span><button aria-label={`Remove ${path}`} data-agment={i} onClick={() => removeMention(i)}><Icon name="x" /></button></span>)}
    {MN.uploads.map(upload => <span className="agment up" key={key(upload)} title={`Uploading to ${machine}`}><Spinner /><span className="nm">{upload.name}</span><small>{upload.size ? Math.round(upload.done / upload.size * 100) : 0}%</small></span>)}
    {ask && <div className="agupask"><span>Upload {ask.files.length === 1 ? <b>{ask.files[0].name}</b> : `${ask.files.length} files`} ({shortSize(ask.files.reduce((n, f) => n + f.size, 0))}) to {machine}:</span><Button ref={there} look="primary" data-agup="there" onClick={() => answerUpload("there")}>Into {ask.folder ? `${ask.folder}/` : "the session's folder"}</Button><Button data-agup="default" title="Outside the project. Deleted when this session is closed." onClick={() => answerUpload("default")}>Temporary attachments</Button><Button look="quiet" data-agup="cancel" onClick={() => answerUpload("cancel")}>Cancel</Button></div>}
    {AG.attError && <span className="derr">{AG.attError}</span>}</>;
}
