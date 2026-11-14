import { AttachmentBatch, ImagePreview } from "./composer-ui.tsx";
import { identity } from "../ui/parts.tsx";
import { agentMessage } from "./session-frame.tsx";
import { Button, Spinner } from "../ui/kit.tsx";
import { useLayoutEffect, useRef } from "react";
import { shortSize } from "../agentpane/files-state.ts";
import { paneSession } from "../agentpane/media.ts";
import { Icon } from "../ui/icons.tsx";
import { AG, folderName } from "./agents.ts";
import { drawAgentAttached } from "./images.tsx";
import { answerUpload, MN, removeMention } from "./workdrop.ts";

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
  const loose = MN.paths.filter(path => !AG.att.some(image => image.original === path));
  return <><AttachmentBatch count={AG.att.length + loose.length + MN.uploads.length}>{AG.att.map((image, i) => <span className="att" key={identity(image)}><ImagePreview images={AG.att} index={i} listed /><button type="button" aria-label={`Remove ${image.name}`} data-agatt={i} onClick={() => removeImage(i)}><Icon name="x" /></button></span>)}
    {MN.paths.map((path, i) => {
      if (AG.att.some(image => image.original === path)) return null;
      const file = MN.files.get(path);
      const url = file?.mime.startsWith("image/") ? `data:${file.mime};base64,${file.data}` : null;
      const name = file?.name ?? folderName(path);

      return <span className={url ? "att" : "agment"} key={path} title={path}>
        {url ? <ImagePreview images={[{ url, name }]} index={0} listed={false} /> : <><Icon name="doc" /><span className="nm">{name}</span></>}
        <button type="button" aria-label={`Remove ${name}`} data-agment={i} onClick={() => removeMention(i)}><Icon name="x" /></button>
      </span>;
    })}
    {MN.uploads.map(upload => <span className="agment up" key={identity(upload)} title={`Uploading to ${machine}`}><Spinner /><span className="nm">{upload.name}</span><small>{upload.size ? Math.round(upload.done / upload.size * 100) : 0}%</small></span>)}
    </AttachmentBatch>
    {ask && <div className="agupask"><span>Upload {ask.files.length === 1 ? <b>{ask.files[0].name}</b> : `${ask.files.length} files`} ({shortSize(ask.files.reduce((n, f) => n + f.size, 0))}) to {machine}:</span><Button ref={there} look="primary" data-agup="there" onClick={() => answerUpload("there")}>Into {ask.folder ? `${ask.folder}/` : "the session's folder"}</Button><Button data-agup="default" title="Outside the project. Deleted when this session is closed." onClick={() => answerUpload("default")}>Temporary attachments</Button><Button look="quiet" data-agup="cancel" onClick={() => answerUpload("cancel")}>Cancel</Button></div>}
    {AG.attError && <span className="derr">{AG.attError}</span>}</>;
}
