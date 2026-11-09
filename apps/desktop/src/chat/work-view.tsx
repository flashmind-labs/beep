import { createElement, Fragment, memo, useEffect, useRef, useState } from "react";
import { createPortal, flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { COPY_ICON, ICON } from "../ui/icons.ts";
import { PLAY_ICON } from "../lists/memories.ts";
import type { ApprovalField, Decision } from "./chat.ts";
import { invoke } from "../ui/tauri.ts";
import type { WebSource } from "./history.ts";
import { liveText } from "../agents/karaoke.ts";
import { duration, thoughtLabel } from "../agents/agentturns.ts";
import { KIND_ICONS } from "../agents/turnrows.ts";
import type { ToolKind } from "../agents/agentturns.ts";
import { AgentTurnHeading, type TurnHeading } from "../agents/turn-heading-view.tsx";
import { RouteView } from "./route-view.tsx";
import { ChartView } from "./chart-view.tsx";
import type { Chart, ChartCard } from "./charts.ts";
import { fileKind, fileSize, GONE, GONE_PIC, loadPic, openViewer, pictures, savePic, type PicRef, type SentFile, type SentImage } from "./media.ts";
import { VOICE_ICON } from "../bar/live.ts";
import type { AskFile } from "./attach.ts";
import type { ChatBody, Turn } from "../ui/turn.ts";

export interface WorkStep { key: string; kind: ToolKind; verb: string; target: string; label: string; state: "run" | "ok" | "bad" | "stop" }
export interface WorkThought { key: string; text: string; ms: number | null; live: boolean; open: boolean; folding: boolean }
export interface WorkLive { thinking: boolean; start: number | null }
export interface WorkView { heading: TurnHeading | null; thought: WorkThought | null; steps: WorkStep[]; live: WorkLive | null }
type Source = WebSource & { site: string };
interface ReplyActions { copy: boolean; rerun: boolean; agent: string | null }
interface ApprovalView { app: string; action: string; fields: ApprovalField[]; grant: boolean; answer: (decision: Decision) => void }
type StatusNotice = { kind: "label"; message: string } | { kind: "error"; message: string } | { kind: "stopped"; message: string; retry: boolean };
interface TurnFrame { prompt: boolean; pictures: boolean }
interface Owner { routes: Map<HTMLElement, { key: number; callId: string }>; frames: Map<HTMLElement, { key: number; value: TurnFrame; setTurn(element: HTMLDivElement | null): void }>; host: HTMLElement | null; body: ChatBody | null; setBody(element: HTMLDivElement | null): void; charts: Map<ChartCard, { key: number; chart: Chart }>; notices: Map<HTMLElement, { key: number; value: StatusNotice }>; products: Map<HTMLElement, { key: number; html: string }>; replyPictures: Map<HTMLElement, { key: number; images: SentImage[] }>; sentFiles: Map<HTMLElement, { key: number; file: SentFile }>; pictures: Map<HTMLElement, { key: number; images: PicRef[] }>; files: Map<HTMLElement, { key: number; files: AskFile[] }>; root: Root; sources: Map<HTMLElement, { key: number; items: Source[] }>; actions: Map<HTMLElement, { key: number; value: ReplyActions }>; approvals: Map<HTMLElement, { key: number; value: ApprovalView }>; prompts: Map<HTMLElement, { key: number; html: string }>; suggestions: Map<HTMLElement, { key: number; replies: string[]; send: (text: string) => void }>; rows: Map<HTMLElement, { key: number; value: WorkView }>; replies: Map<HTMLElement, { key: number; html: string }>; next: number }
// A retained chat body owns all its portals, including detached turns while
// history is merged. Transfer moved hosts before discarding the fresh turn.
const owners = new WeakMap<ChatBody, Owner>();
const turnHosts = new WeakMap<Turn, { host: HTMLElement; owner: Owner }>();
const routeTurns = new WeakMap<Turn, { owner: Owner; boxes: HTMLElement[] }>();
const chartTurns = new WeakMap<Turn, { owner: Owner; boxes: ChartCard[] }>();
const noticeTurns = new WeakMap<Turn, { owner: Owner; boxes: HTMLElement[] }>();
const productTurns = new WeakMap<Turn, { owner: Owner; boxes: HTMLElement[] }>();
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
function Notice({ value }: { value: StatusNotice }) {
  if (value.kind === "label") return <>{value.message}</>;
  if (value.kind === "error") return <><b>The turn did not finish</b><span>{value.message}</span></>;
  return <>{value.message}{value.retry && <button className="dbtn" data-act="retry">Retry</button>}</>;
}
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
const WorkStepRow = memo(function WorkStepRow({ step }: { step: WorkStep }) {
  return <div className="dcoderen tool row"><span className="ki" dangerouslySetInnerHTML={{ __html: KIND_ICONS[step.kind] }} /><span className="vb">{step.verb}</span><code className="tt" title={step.label}>{step.target}</code><span className={`rm${step.state === "bad" ? " bad" : ""}`}>{step.state === "bad" ? "failed" : ""}</span><span className="mk">{step.state === "run" ? <span className="dspin" title="Running" /> : step.state === "stop" ? <span className="ddot off" title="Did not finish" /> : <span className={step.state === "ok" ? "dok" : "dbad"} title={step.state === "ok" ? "Done" : "Failed"} dangerouslySetInnerHTML={{ __html: step.state === "ok" ? ICON.check : ICON.x }} />}</span></div>;
});
function Thought({ value }: { value: WorkThought }) {
  const text = value.live || value.folding ? liveText(value.text) : value.text;
  return <div className={`dthink${value.live ? " live" : ""}${value.open ? " open" : ""}`} data-folding={value.folding ? "" : undefined}><button data-thought={value.key} aria-expanded={value.open}><span className="lb">{thoughtLabel(value.ms)}</span><span className="cv" dangerouslySetInnerHTML={{ __html: ICON.chev }} /></button><div className="bd"><div>{value.live || value.folding ? <div className="tx"><div className={`kk${value.folding ? " ended" : ""}`} data-kk={value.live ? value.key : undefined}>{text}</div></div> : <div className="tx md">{text.split("\n").map((line, index) => <Fragment key={index}>{index > 0 && <br />}{line}</Fragment>)}</div>}</div></div></div>;
}
function LiveLine({ value }: { value: WorkLive }) {
  const lead = value.thinking ? "Thinking" : "Working";
  return <div className="dthink dlive live"><span className="lb" title={lead}>{lead}</span>{value.start !== null && <span className="since" data-since={value.start}>{duration(Date.now() - value.start)}</span>}</div>;
}
const Work = memo(function Work({ value }: { value: WorkView }) {
  return <>{value.heading && <AgentTurnHeading value={value.heading} />}{value.thought && <Thought value={value.thought} />}{value.steps.map(step => <WorkStepRow key={step.key} step={step} />)}{value.live && <LiveLine value={value.live} />}</>;
});
function draw(owner: Owner) {
  flushSync(() => owner.root.render(createElement(Fragment, null,
    owner.host ? createElement("div", { key: "body", className: "dbody", id: "body", ref: owner.setBody }) : null,
    ...[...owner.frames].map(([host, row]) => createPortal(createElement("div", { className: "turn dcoderlog", ref: row.setTurn }, row.value.prompt ? createElement("div", { className: "dcoderen user", key: "prompt" }) : null, row.value.pictures ? createElement("div", { className: "dpics", key: "pictures" }) : null, createElement("template", { key: "anchor", "data-chat-anchor": "" })), host, row.key)),
    ...[...owner.routes].map(([host, row]) => createPortal(createElement(RouteView, { callId: row.callId }), host, row.key)),
    ...[...owner.charts].map(([host, row]) => createPortal(createElement(ChartView, { card: host, initial: row.chart }), host, row.key)),
    ...[...owner.notices].map(([host, row]) => createPortal(createElement(Notice, { value: row.value }), host, row.key)),
    ...[...owner.products].map(([host, row]) => createPortal(createElement(Html, { html: row.html }), host, row.key)),
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
function makeOwner(host: HTMLElement | null): Owner {
  const container = host ?? document.createElement("div");
  const owner: Owner = { routes: new Map(), frames: new Map(), host, body: null, setBody: element => { owner.body = element; }, charts: new Map(), notices: new Map(), products: new Map(), replyPictures: new Map(), sentFiles: new Map(), pictures: new Map(), files: new Map(), root: createRoot(container), sources: new Map(), actions: new Map(), approvals: new Map(), prompts: new Map(), suggestions: new Map(), rows: new Map(), replies: new Map(), next: 0 };
  return owner;
}
function ownerOf(body: ChatBody) {
  let owner = owners.get(body);
  if (!owner) { owner = makeOwner(null); owners.set(body, owner); }
  return owner;
}
export function createChatBody(): ChatBody {
  const host = document.createElement("div");
  host.style.display = "contents";
  const owner = makeOwner(host);
  draw(owner);
  if (!owner.body) throw new Error("Chat body did not mount");
  owners.set(owner.body, owner);
  return owner.body;
}
export function chatBodyHost(body: ChatBody): HTMLElement {
  return owners.get(body)?.host ?? body;
}

// History creates frames in one render; their content portals can then update
// independently while the conversation moves the display-contents hosts.
export function createChatTurns(body: ChatBody, values: readonly TurnFrame[]): Turn[] {
  if (!values.length) return [];
  const owner = ownerOf(body);
  const frames = values.map(value => {
    const host = document.createElement("div");
    host.style.display = "contents";
    const frame: { host: HTMLElement; turn: Turn | null } = { host, turn: null };
    owner.frames.set(host, { key: ++owner.next, value, setTurn: element => { frame.turn = element; } });
    return frame;
  });
  draw(owner);
  return frames.map(frame => {
    if (!frame.turn) throw new Error("Chat turn did not mount");
    turnHosts.set(frame.turn, { host: frame.host, owner });
    return frame.turn;
  });
}
export function createChatTurn(body: ChatBody, value: TurnFrame): Turn {
  const turn = createChatTurns(body, [value])[0];
  if (!turn) throw new Error("Chat turn did not mount");
  return turn;
}

export function ensureChatPrompt(turn: Turn): HTMLElement | null {
  const entry = turnHosts.get(turn), frame = entry?.owner.frames.get(entry.host);
  if (!entry || !frame) return null;
  if (!frame.value.prompt) {
    frame.value = { ...frame.value, prompt: true };
    draw(entry.owner);
  }
  return turn.querySelector<HTMLElement>(":scope > .dcoderen.user");
}
export function chatTurnHost(turn: Turn): HTMLElement {
  return turnHosts.get(turn)?.host ?? turn;
}
export function renderChatRoute(body: ChatBody, turn: Turn, box: HTMLElement, callId: string) {
  const owner = ownerOf(body), entry = routeTurns.get(turn);
  if (entry) entry.boxes.push(box); else routeTurns.set(turn, { owner, boxes: [box] });
  owner.routes.set(box, { key: ++owner.next, callId });
  draw(owner);
}
export function renderChatChart(body: ChatBody, turn: Turn, box: ChartCard, chart: Chart) {
  const owner = ownerOf(body), entry = chartTurns.get(turn);
  if (entry) entry.boxes.push(box); else chartTurns.set(turn, { owner, boxes: [box] });
  owner.charts.set(box, { key: ++owner.next, chart });
  draw(owner);
}
export function renderChatNotice(body: ChatBody, turn: Turn, box: HTMLElement, value: StatusNotice) {
  const owner = ownerOf(body), entry = noticeTurns.get(turn);
  if (entry) entry.boxes.push(box); else noticeTurns.set(turn, { owner, boxes: [box] });
  owner.notices.set(box, { key: ++owner.next, value });
  draw(owner);
}
export function discardChatNotice(turn: Turn, box: HTMLElement) {
  const entry = noticeTurns.get(turn);
  if (!entry) return;
  entry.boxes = entry.boxes.filter(item => item !== box);
  if (!entry.boxes.length) noticeTurns.delete(turn);
  entry.owner.notices.delete(box);
  draw(entry.owner);
}
export function transferChatNotice(from: Turn, to: Turn, box: HTMLElement) {
  const entry = noticeTurns.get(from);
  if (!entry || !entry.boxes.includes(box)) return;
  entry.boxes = entry.boxes.filter(item => item !== box);
  if (!entry.boxes.length) noticeTurns.delete(from);
  const target = noticeTurns.get(to);
  if (target) target.boxes.push(box); else noticeTurns.set(to, { owner: entry.owner, boxes: [box] });
}
export function renderChatProduct(body: ChatBody, turn: Turn, box: HTMLElement, html: string) {
  const owner = ownerOf(body), entry = productTurns.get(turn);
  if (entry) entry.boxes.push(box); else productTurns.set(turn, { owner, boxes: [box] });
  owner.products.set(box, { key: ++owner.next, html });
  draw(owner);
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
  const nextRoutes = routeTurns.get(from), priorRoutes = routeTurns.get(to);
  routeTurns.delete(from);
  if (priorRoutes) for (const box of priorRoutes.boxes) priorRoutes.owner.routes.delete(box);
  if (nextRoutes) routeTurns.set(to, nextRoutes); else routeTurns.delete(to);
  if (priorRoutes && priorRoutes.owner !== nextRoutes?.owner) draw(priorRoutes.owner);
  const nextCharts = chartTurns.get(from), priorCharts = chartTurns.get(to);
  chartTurns.delete(from);
  if (priorCharts) for (const box of priorCharts.boxes) priorCharts.owner.charts.delete(box);
  if (nextCharts) chartTurns.set(to, nextCharts); else chartTurns.delete(to);
  if (priorCharts && priorCharts.owner !== nextCharts?.owner) draw(priorCharts.owner);
  const nextProducts = productTurns.get(from), priorProducts = productTurns.get(to);
  productTurns.delete(from);
  if (priorProducts) for (const box of priorProducts.boxes) priorProducts.owner.products.delete(box);
  if (nextProducts) productTurns.set(to, nextProducts); else productTurns.delete(to);
  if (priorProducts && priorProducts.owner !== nextProducts?.owner) draw(priorProducts.owner);
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
  const owner = next?.owner ?? prior?.owner ?? nextPictures?.owner ?? priorPictures?.owner ?? nextProducts?.owner ?? priorProducts?.owner ?? nextCharts?.owner ?? priorCharts?.owner ?? nextRoutes?.owner ?? priorRoutes?.owner;
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
  const frame = turnHosts.get(turn);
  turnHosts.delete(turn);
  if (frame) frame.owner.frames.delete(frame.host);
  const routes = routeTurns.get(turn), charts = chartTurns.get(turn), notices = noticeTurns.get(turn), products = productTurns.get(turn), replyPictures = replyPictureTurns.get(turn), sentFiles = sentFileTurns.get(turn), pictures = pictureTurns.get(turn), files = fileTurns.get(turn), sources = sourceTurns.get(turn), actions = actionTurns.get(turn), approval = approvalTurns.get(turn), suggestion = suggestionTurns.get(turn), prompt = promptTurns.get(turn), work = turns.get(turn), reply = replyTurns.get(turn);
  routeTurns.delete(turn); chartTurns.delete(turn); noticeTurns.delete(turn); productTurns.delete(turn); replyPictureTurns.delete(turn); sentFileTurns.delete(turn); pictureTurns.delete(turn); fileTurns.delete(turn); sourceTurns.delete(turn); actionTurns.delete(turn); approvalTurns.delete(turn); suggestionTurns.delete(turn); promptTurns.delete(turn); turns.delete(turn); replyTurns.delete(turn);
  if (routes) for (const box of routes.boxes) routes.owner.routes.delete(box);
  if (charts) for (const box of charts.boxes) charts.owner.charts.delete(box);
  if (notices) for (const box of notices.boxes) notices.owner.notices.delete(box);
  if (products) for (const box of products.boxes) products.owner.products.delete(box);
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
  const owners = new Set([frame?.owner, routes?.owner, charts?.owner, notices?.owner, products?.owner, replyPictures?.owner, sentFiles?.owner, pictures?.owner, files?.owner, sources?.owner, actions?.owner, approval?.owner, suggestion?.owner, prompt?.owner, work?.owner, reply?.owner]);
  for (const owner of owners) if (owner) draw(owner);
}
