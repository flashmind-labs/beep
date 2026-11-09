import { createElement, Fragment, memo, useEffect, useRef, useState } from "react";
import { createPortal, flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { COPY_ICON, ICON } from "../ui/icons.ts";
import { PLAY_ICON } from "../lists/memories.ts";
import type { ApprovalField, Decision } from "./chat.ts";
import { invoke } from "../ui/tauri.ts";
import type { WebSource } from "./history.ts";
import { AgentTurnHeading, type TurnHeading } from "../agents/turn-heading-view.tsx";
import { fileKind, fileSize, GONE, GONE_PIC, loadPic, openViewer, pictures, savePic, type PicRef, type SentFile, type SentImage } from "./media.ts";
import { VOICE_ICON } from "../bar/live.ts";
import type { AskFile } from "./attach.ts";
import type { ChatBody, Turn } from "../ui/turn.ts";

interface Part { key: string; html: string }
export interface WorkView { heading: TurnHeading | null; thought: string; steps: Part[]; live: string }
type Source = WebSource & { site: string };
interface ReplyActions { copy: boolean; rerun: boolean; agent: string | null }
interface ApprovalView { app: string; action: string; fields: ApprovalField[]; grant: boolean; answer: (decision: Decision) => void }
interface Owner { replyPictures: Map<HTMLElement, { key: number; images: SentImage[] }>; sentFiles: Map<HTMLElement, { key: number; file: SentFile }>; pictures: Map<HTMLElement, { key: number; images: PicRef[] }>; files: Map<HTMLElement, { key: number; files: AskFile[] }>; root: Root; sources: Map<HTMLElement, { key: number; items: Source[] }>; actions: Map<HTMLElement, { key: number; value: ReplyActions }>; approvals: Map<HTMLElement, { key: number; value: ApprovalView }>; prompts: Map<HTMLElement, { key: number; html: string }>; suggestions: Map<HTMLElement, { key: number; replies: string[]; send: (text: string) => void }>; rows: Map<HTMLElement, { key: number; value: WorkView }>; replies: Map<HTMLElement, { key: number; html: string }>; next: number }
const owners = new WeakMap<ChatBody, Owner>();
const replyPictureTurns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const sentFileTurns = new WeakMap<Turn, { owner: Owner; boxes: HTMLElement[] }>();
const pictureTurns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const fileTurns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const sourceTurns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const actionTurns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const approvalTurns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const suggestionTurns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const promptTurns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const turns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const replyTurns = new WeakMap<Turn, { owner: Owner; box: HTMLElement }>();
const promptPending = new Map<HTMLElement, { html: string; resume: () => void }>();
const replyPending = new Map<HTMLElement, { html: string; resume: () => void }>();
const pending = new Map<HTMLElement, { value: WorkView; after: () => void; resume: () => void }>();

const Html = memo(function Html({ html }: { html: string }) {
  return <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: html }} />;
});
const Prompt = memo(function Prompt({ html }: { html: string }) {
  return <><b>You</b><span dangerouslySetInnerHTML={{ __html: html }} /></>;
});
function ReplyPicture({ image, images, index }: { image: SentImage; images: SentImage[]; index: number }) {
  const file = image.file;
  const [src, setSrc] = useState<string | null>(file ? null : image.url);
  const [gone, setGone] = useState(false), [waiting, setWaiting] = useState(true);
  useEffect(() => {
    if (!file) return;
    let active = true;
    void loadPic(file.id, file.mime).then(url => { if (active) { if (url) setSrc(url); else setGone(true); } });
    return () => { active = false; };
  }, [file?.id, file?.mime]);
  if (gone) return <span className="drpic gone">{GONE_PIC}</span>;
  const name = file?.name ?? "";
  return <span className={`drpic${waiting ? " wait" : ""}`}><button className="pic" aria-label={image.caption || name || "Picture"} onClick={() => openViewer(images, index)}>{src && <img alt="" src={src} onLoad={() => setWaiting(false)} onError={() => setGone(true)} />}</button>{file && <button className="save" aria-label={`Save ${name}`} title="Save to Downloads" onClick={() => void savePic(file)} dangerouslySetInnerHTML={{ __html: ICON.save }} />}</span>;
}
const ReplyPictures = memo(function ReplyPictures({ images }: { images: SentImage[] }) {
  return <>{images.map((image, index) => <ReplyPicture key={index} image={image} images={images} index={index} />)}</>;
});
function SentFileCard({ file }: { file: SentFile }) {
  const busy = useRef(false);
  const [status, setStatus] = useState<{ kind: "idle" } | { kind: "busy" } | { kind: "error"; message: string }>({ kind: "idle" });
  const about = [fileKind(file), fileSize(file.size)].filter(Boolean).join(", ");
  const gone = status.kind === "error" && status.message === GONE;
  const text = status.kind === "busy" ? "Downloading…" : status.kind === "error" ? gone ? status.message : `${about}. ${status.message}` : about;
  const open = () => {
    if (busy.current || gone) return;
    busy.current = true;
    setStatus({ kind: "busy" });
    void invoke("open_file", { id: file.id, name: file.name }).then(() => setStatus({ kind: "idle" }), e => setStatus({ kind: "error", message: String(e) })).finally(() => { busy.current = false; });
  };
  return <button className={`dfile${gone ? " gone" : ""}`} onClick={open}><span className="ic" dangerouslySetInnerHTML={{ __html: ICON.doc }} /><span className="dcol"><span className="nm">{file.name}</span><span className="sz">{text}</span></span>{!gone && <span className="go">{status.kind === "busy" ? <span className="dspin" /> : <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.chev }} />}</span>}</button>;
}
function Picture({ image }: { image: PicRef }) {
  const id = "id" in image ? image.id : null;
  const mime = "id" in image ? image.mime : null;
  const cached = "url" in image ? image.url : pictures.get(image.id);
  const [loaded, setLoaded] = useState<string | null | Promise<string | null> | undefined>(cached);
  useEffect(() => {
    if (id === null || mime === null || typeof cached === "string" || cached === null) return;
    let active = true;
    void loadPic(id, mime).then(url => { if (active) setLoaded(url); });
    return () => { active = false; };
  }, [id, mime]);
  if (typeof loaded === "string") return <img className="dpic" src={loaded} alt="" />;
  if (loaded === null) return <span className="dpic gone">{GONE_PIC}</span>;
  return <span className="dpic wait" data-id={id} data-mime={mime} />;
}
const PromptPictures = memo(function PromptPictures({ images }: { images: PicRef[] }) {
  return <>{images.map((image, i) => <Picture key={i} image={image} />)}</>;
});
const FileChip = memo(function FileChip({ file }: { file: AskFile }) {
  const [error, setError] = useState<string | null>(null);
  const audio = file.mime?.startsWith("audio/");
  const about = [audio ? "Audio" : fileKind(file), fileSize(file.size)].filter(Boolean).join(", ");
  const content = <><span className="ic" dangerouslySetInnerHTML={{ __html: audio ? VOICE_ICON.mic : ICON.doc }} /><span className="dcol0"><span className="nm">{audio ? "Your recording" : file.name}</span><span className="sz">{error ?? about}</span></span></>;
  if (!file.id) return <span className="dachip" aria-label={audio ? "Open your recording" : undefined}>{content}</span>;
  return <button className={`dachip${error === GONE ? " gone" : ""}`} aria-label={audio ? "Open your recording" : undefined} onClick={() => { if (error !== GONE) void invoke("open_file", { id: file.id, name: file.name }).catch(e => setError(String(e))); }}>{content}</button>;
});
const Files = memo(function Files({ files }: { files: AskFile[] }) {
  return <>{files.map((file, i) => <FileChip key={i} file={file} />)}</>;
});
const Sources = memo(function Sources({ items }: { items: Source[] }) {
  const [open, setOpen] = useState(false);
  const shown = open ? items : items.slice(0, 3), more = items.length - shown.length;
  return <>{shown.map((source, i) => <button key={`${source.url}:${i}`} className="dsrc" data-i={i} title={source.title || source.url} onClick={() => void invoke("open_url", { url: source.url }).catch(() => {})}><i>{source.site[0].toUpperCase()}</i><span>{source.site}</span></button>)}{more > 0 && <button className="dsrc more" aria-label={`Show ${more} more sources`} onClick={() => setOpen(true)}>+{more}</button>}</>;
});
const ReplyActionsView = memo(function ReplyActionsView({ value }: { value: ReplyActions }) {
  return <>{value.copy && <button className="dcopy" data-act="copyReply"><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: COPY_ICON }} />Copy</button>}{value.rerun && <span className="drerun"><button className="dcopy" data-act="rerunMenu" aria-haspopup="menu" aria-expanded={false}><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: PLAY_ICON }} />Rerun<span className="dchev" dangerouslySetInnerHTML={{ __html: ICON.chev }} /></button></span>}{value.agent && <span className="dagent" title="The agent that answered">{value.agent}</span>}</>;
});
const Approval = memo(function Approval({ value }: { value: ApprovalView }) {
  return <><div className="head"><span className="dapp" data-app={value.app} /><div className="dcol"><b>{value.action}?</b><small>{value.app} asks first</small></div></div>
    {!!value.fields.length && <div className="kv">{value.fields.map((field, i) => <Fragment key={i}><span>{field.label}</span><span>{field.value}</span></Fragment>)}</div>}
    <div className="acts"><button className="dbtn pri" data-d="allow" onClick={() => value.answer("allow")}>Allow<kbd>⌘<span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.ret }} /></kbd></button>{!value.grant && <><button className="dbtn" data-d="session" title="Allow this action without asking until this chat ends" onClick={() => value.answer("session")}>Allow for this session<kbd>⇧⌘<span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.ret }} /></kbd></button><button className="dbtn" data-d="always" title="Allow this action without asking until this chat ends" onClick={() => value.answer("always")}>Always<kbd>⌥⌘<span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.ret }} /></kbd></button></>}<button className="dbtn warn" data-d="deny" onClick={() => value.answer("deny")}>Deny<kbd>⌘D</kbd></button></div></>;
});
const Suggestions = memo(function Suggestions({ replies, send }: { replies: string[]; send: (text: string) => void }) {
  return <>{replies.map((reply, i) => <button className="dbtn" key={`${i}:${reply}`} onClick={() => send(reply)}>{reply}</button>)}</>;
});
const Work = memo(function Work({ value }: { value: WorkView }) {
  return <>{value.heading && <AgentTurnHeading value={value.heading} />}{value.thought && <Html html={value.thought} />}{value.steps.map(step => <Html key={step.key} html={step.html} />)}{value.live && <Html html={value.live} />}</>;
});
function draw(owner: Owner) {
  flushSync(() => owner.root.render(createElement(Fragment, null,
    ...[...owner.replyPictures].map(([host, row]) => createPortal(createElement(ReplyPictures, { images: row.images }), host, row.key)),
    ...[...owner.sentFiles].map(([host, row]) => createPortal(createElement(SentFileCard, { file: row.file }), host, row.key)),
    ...[...owner.pictures].map(([host, row]) => createPortal(createElement(PromptPictures, { images: row.images }), host, row.key)),
    ...[...owner.files].map(([host, row]) => createPortal(createElement(Files, { files: row.files }), host, row.key)),
    ...[...owner.sources].map(([host, row]) => createPortal(createElement(Sources, { items: row.items }), host, row.key)),
    ...[...owner.actions].map(([host, row]) => createPortal(createElement(ReplyActionsView, { value: row.value }), host, row.key)),
    ...[...owner.approvals].map(([host, row]) => createPortal(createElement(Approval, { value: row.value }), host, row.key)),
    ...[...owner.suggestions].map(([host, row]) => createPortal(createElement(Suggestions, { replies: row.replies, send: row.send }), host, row.key)),
    ...[...owner.prompts].map(([host, row]) => createPortal(createElement(Prompt, { html: row.html }), host, row.key)),
    ...[...owner.rows].map(([host, row]) => createPortal(createElement(Work, { value: row.value }), host, row.key)),
    ...[...owner.replies].map(([host, row]) => createPortal(createElement(Html, { html: row.html }), host, row.key)))));
}
function ownerOf(body: ChatBody) {
  let owner = owners.get(body);
  if (!owner) { owner = { replyPictures: new Map(), sentFiles: new Map(), pictures: new Map(), files: new Map(), root: createRoot(document.createElement("div")), sources: new Map(), actions: new Map(), approvals: new Map(), prompts: new Map(), suggestions: new Map(), rows: new Map(), replies: new Map(), next: 0 }; owners.set(body, owner); }
  return owner;
}
export function renderChatReplyPictures(body: ChatBody, turn: Turn, box: HTMLElement, images: SentImage[]) {
  const owner = ownerOf(body), prior = owner.replyPictures.get(box);
  replyPictureTurns.set(turn, { owner, box });
  owner.replyPictures.set(box, { key: prior?.key ?? ++owner.next, images: images.slice() });
  draw(owner);
}
export function renderChatSentFile(body: ChatBody, turn: Turn, box: HTMLElement, file: SentFile) {
  const owner = ownerOf(body), entry = sentFileTurns.get(turn);
  if (entry) entry.boxes.push(box); else sentFileTurns.set(turn, { owner, boxes: [box] });
  owner.sentFiles.set(box, { key: ++owner.next, file });
  draw(owner);
}
export function transferChatSentFiles(from: Turn, to: Turn) {
  const nextPictures = replyPictureTurns.get(from), priorPictures = replyPictureTurns.get(to);
  replyPictureTurns.delete(from);
  if (priorPictures) priorPictures.owner.replyPictures.delete(priorPictures.box);
  if (nextPictures) replyPictureTurns.set(to, nextPictures); else replyPictureTurns.delete(to);
  if (priorPictures && priorPictures.owner !== nextPictures?.owner) draw(priorPictures.owner);
  const next = sentFileTurns.get(from), prior = sentFileTurns.get(to);
  sentFileTurns.delete(from);
  if (prior) for (const box of prior.boxes) prior.owner.sentFiles.delete(box);
  if (next) sentFileTurns.set(to, next); else sentFileTurns.delete(to);
  if (prior && prior.owner !== next?.owner) draw(prior.owner);
  const owner = next?.owner ?? prior?.owner ?? nextPictures?.owner ?? priorPictures?.owner;
  if (owner) draw(owner);
}
export function renderChatPictures(body: ChatBody, turn: Turn, box: HTMLElement, images: PicRef[]) {
  const owner = ownerOf(body);
  pictureTurns.set(turn, { owner, box });
  owner.pictures.set(box, { key: ++owner.next, images });
  draw(owner);
}
export function transferChatPictures(from: Turn, to: Turn) {
  const entry = pictureTurns.get(from);
  if (!entry) return;
  pictureTurns.delete(from); pictureTurns.set(to, entry);
}
export function renderChatFiles(body: ChatBody, turn: Turn, box: HTMLElement, files: AskFile[]) {
  const owner = ownerOf(body);
  fileTurns.set(turn, { owner, box });
  owner.files.set(box, { key: ++owner.next, files });
  draw(owner);
}
export function transferChatFiles(from: Turn, to: Turn) {
  const entry = fileTurns.get(from);
  if (!entry) return;
  fileTurns.delete(from); fileTurns.set(to, entry);
}
export function renderChatSources(body: ChatBody, turn: Turn, box: HTMLElement, items: Source[]) {
  const owner = ownerOf(body), prior = owner.sources.get(box);
  sourceTurns.set(turn, { owner, box });
  owner.sources.set(box, { key: prior?.key ?? ++owner.next, items });
  draw(owner);
}
export function discardChatSources(turn: Turn) {
  const entry = sourceTurns.get(turn);
  sourceTurns.delete(turn);
  if (entry) { entry.owner.sources.delete(entry.box); draw(entry.owner); }
}
export function renderChatActions(body: ChatBody, turn: Turn, box: HTMLElement, value: ReplyActions) {
  const owner = ownerOf(body), prior = owner.actions.get(box);
  actionTurns.set(turn, { owner, box });
  owner.actions.set(box, { key: prior?.key ?? ++owner.next, value });
  draw(owner);
}
export function discardChatActions(turn: Turn) {
  const entry = actionTurns.get(turn);
  actionTurns.delete(turn);
  if (entry) { entry.owner.actions.delete(entry.box); draw(entry.owner); }
}
export function renderChatApproval(body: ChatBody, turn: Turn, box: HTMLElement, value: ApprovalView) {
  const owner = ownerOf(body), prior = owner.approvals.get(box);
  approvalTurns.set(turn, { owner, box });
  owner.approvals.set(box, { key: prior?.key ?? ++owner.next, value });
  draw(owner);
}
export function discardChatApproval(turn: Turn) {
  const entry = approvalTurns.get(turn);
  approvalTurns.delete(turn);
  if (entry) { entry.owner.approvals.delete(entry.box); draw(entry.owner); }
}
export function transferChatApproval(from: Turn, to: Turn) {
  const next = approvalTurns.get(from), prior = approvalTurns.get(to);
  approvalTurns.delete(from);
  if (prior) prior.owner.approvals.delete(prior.box);
  if (next) approvalTurns.set(to, next); else approvalTurns.delete(to);
  if (prior && prior.owner !== next?.owner) draw(prior.owner);
  const current = next ?? prior;
  if (current) draw(current.owner);
}
export function renderChatSuggestions(body: ChatBody, turn: Turn, box: HTMLElement, replies: string[], send: (text: string) => void) {
  const owner = ownerOf(body), prior = owner.suggestions.get(box);
  suggestionTurns.set(turn, { owner, box });
  owner.suggestions.set(box, { key: prior?.key ?? ++owner.next, replies, send });
  draw(owner);
}
export function discardChatSuggestions(turn: Turn) {
  const entry = suggestionTurns.get(turn);
  suggestionTurns.delete(turn);
  if (entry) { entry.owner.suggestions.delete(entry.box); draw(entry.owner); }
}
export function transferChatSuggestions(from: Turn, to: Turn) {
  const next = suggestionTurns.get(from), prior = suggestionTurns.get(to);
  suggestionTurns.delete(from);
  if (prior) prior.owner.suggestions.delete(prior.box);
  if (next) suggestionTurns.set(to, next); else suggestionTurns.delete(to);
  if (prior && prior.owner !== next?.owner) draw(prior.owner);
  const current = next ?? prior;
  if (current) draw(current.owner);
}
export function renderChatPrompt(body: ChatBody, turn: Turn, box: HTMLElement, html: string) {
  const owner = ownerOf(body);
  promptTurns.set(turn, { owner, box });
  owner.prompts.set(box, { key: ++owner.next, html });
  draw(owner);
}
export function updateChatPrompt(turn: Turn, html: string) {
  const entry = promptTurns.get(turn);
  if (!entry) return false;
  const prior = entry.owner.prompts.get(entry.box);
  if (!prior) return false;
  const selected = () => { const selection = document.getSelection(); return !!selection && !selection.isCollapsed && selection.containsNode(entry.box, true); };
  const apply = (value: string) => { entry.owner.prompts.set(entry.box, { ...prior, html: value }); draw(entry.owner); };
  if (selected()) {
    const waiting = promptPending.get(entry.box);
    if (waiting) { waiting.html = html; return true; }
    const resume = () => {
      const latest = promptPending.get(entry.box);
      if (!latest || selected()) return;
      promptPending.delete(entry.box); document.removeEventListener("selectionchange", resume); apply(latest.html);
    };
    promptPending.set(entry.box, { html, resume }); document.addEventListener("selectionchange", resume);
    return true;
  }
  const waiting = promptPending.get(entry.box);
  if (waiting) { document.removeEventListener("selectionchange", waiting.resume); promptPending.delete(entry.box); }
  apply(html);
  return true;
}
export function transferChatPrompt(from: Turn, to: Turn) {
  const entry = promptTurns.get(from);
  if (!entry) return;
  promptTurns.delete(from); promptTurns.set(to, entry);
}
export function renderChatWork(body: ChatBody, turn: Turn, box: HTMLElement, value: WorkView, after: () => void) {
  const current = ownerOf(body);
  const selected = () => {
    const selection = document.getSelection();
    return !!selection && !selection.isCollapsed && selection.containsNode(box, true);
  };
  const prior = current.rows.get(box);
  const apply = (next: WorkView, done: () => void) => {
    current.rows.set(box, { key: prior?.key ?? ++current.next, value: next });
    draw(current);
    done();
  };
  turns.set(turn, { owner: current, box });
  if (prior && selected()) {
    const waiting = pending.get(box);
    if (waiting) { waiting.value = value; waiting.after = after; return; }
    const resume = () => {
      const latest = pending.get(box);
      if (!latest || selected()) return;
      pending.delete(box);
      document.removeEventListener("selectionchange", resume);
      apply(latest.value, latest.after);
    };
    pending.set(box, { value, after, resume });
    document.addEventListener("selectionchange", resume);
    return;
  }
  const waiting = pending.get(box);
  if (waiting) { document.removeEventListener("selectionchange", waiting.resume); pending.delete(box); }
  apply(value, after);
}
export function renderChatReply(body: ChatBody, turn: Turn, box: HTMLElement, html: string) {
  const owner = ownerOf(body), prior = owner.replies.get(box);
  replyTurns.set(turn, { owner, box });
  if (prior?.html === html && !replyPending.has(box)) return;
  const selected = () => { const selection = document.getSelection(); return !!selection && !selection.isCollapsed && selection.containsNode(box, true); };
  const apply = (value: string) => { owner.replies.set(box, { key: prior?.key ?? ++owner.next, html: value }); draw(owner); };
  if (prior && selected()) {
    const waiting = replyPending.get(box);
    if (waiting) { waiting.html = html; return; }
    const resume = () => {
      const latest = replyPending.get(box);
      if (!latest || selected()) return;
      replyPending.delete(box); document.removeEventListener("selectionchange", resume); apply(latest.html);
    };
    replyPending.set(box, { html, resume }); document.addEventListener("selectionchange", resume);
    return;
  }
  const waiting = replyPending.get(box);
  if (waiting) { document.removeEventListener("selectionchange", waiting.resume); replyPending.delete(box); }
  apply(html);
}
export function transferChatReply(from: Turn, to: Turn) {
  const entry = replyTurns.get(from);
  if (!entry) return;
  replyTurns.delete(from); replyTurns.set(to, entry);
}
export function discardChatWork(turn: Turn) {
  const replyPictures = replyPictureTurns.get(turn), sentFiles = sentFileTurns.get(turn), pictures = pictureTurns.get(turn), files = fileTurns.get(turn), sources = sourceTurns.get(turn), actions = actionTurns.get(turn), approval = approvalTurns.get(turn), suggestion = suggestionTurns.get(turn), prompt = promptTurns.get(turn), work = turns.get(turn), reply = replyTurns.get(turn);
  replyPictureTurns.delete(turn); sentFileTurns.delete(turn); pictureTurns.delete(turn); fileTurns.delete(turn); sourceTurns.delete(turn); actionTurns.delete(turn); approvalTurns.delete(turn); suggestionTurns.delete(turn); promptTurns.delete(turn); turns.delete(turn); replyTurns.delete(turn);
  if (replyPictures) replyPictures.owner.replyPictures.delete(replyPictures.box);
  if (sentFiles) for (const box of sentFiles.boxes) sentFiles.owner.sentFiles.delete(box);
  if (pictures) pictures.owner.pictures.delete(pictures.box);
  if (files) files.owner.files.delete(files.box);
  if (sources) sources.owner.sources.delete(sources.box);
  if (actions) actions.owner.actions.delete(actions.box);
  if (approval) approval.owner.approvals.delete(approval.box);
  if (suggestion) suggestion.owner.suggestions.delete(suggestion.box);
  if (prompt) {
    const waiting = promptPending.get(prompt.box);
    if (waiting) { document.removeEventListener("selectionchange", waiting.resume); promptPending.delete(prompt.box); }
    prompt.owner.prompts.delete(prompt.box);
  }
  if (work) {
    const waiting = pending.get(work.box);
    if (waiting) { document.removeEventListener("selectionchange", waiting.resume); pending.delete(work.box); }
    work.owner.rows.delete(work.box);
  }
  if (reply) {
    const waiting = replyPending.get(reply.box);
    if (waiting) { document.removeEventListener("selectionchange", waiting.resume); replyPending.delete(reply.box); }
    reply.owner.replies.delete(reply.box);
  }
  const owners = new Set([replyPictures?.owner, sentFiles?.owner, pictures?.owner, files?.owner, sources?.owner, actions?.owner, approval?.owner, suggestion?.owner, prompt?.owner, work?.owner, reply?.owner]);
  for (const owner of owners) if (owner) draw(owner);
}
