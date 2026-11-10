import { AppLogo } from "./applogo-view.tsx";
import { BackgroundView, type BackgroundQuestions } from "./background-view.tsx";
import { Fragment, memo, useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type ReactElement, type ReactNode } from "react";
import { createPortal, flushSync } from "react-dom";
import { createDocumentView, type DocumentView } from "../ui/document-root.tsx";
import { COPY_ICON, ICON, Icon } from "../ui/icons.tsx";
import type { ApprovalField, Decision } from "./approval-cards.ts";
import { invoke } from "../ui/tauri.ts";
import { writeClipboard } from "../ui/parts.ts";
import type { WebSource } from "./history.ts";
import { liveText } from "../agents/karaoke.ts";
import { duration, thoughtLabel } from "../agents/agentturns.ts";
import { KIND_ICONS } from "../agents/agentturns.ts";
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
export interface WorkView { heading: TurnHeading | null; thought: WorkThought | null; steps: WorkStep[]; live: WorkLive | null; toggleThought(open: boolean): void; toggleSteps(): void }
type Source = WebSource & { site: string };
interface RerunChoice { id: string; name: string; detail: string; used: boolean }
interface RerunView { choices: RerunChoice[]; levels: { id: string; label: string }[]; run(model: string, thinking: string | null): void }
interface ReplyActions { copy: boolean; rerun: boolean; agent: string | null }
interface ApprovalView { app: string; action: string; fields: ApprovalField[]; grant: boolean; answer: (decision: Decision) => void }
type StatusNotice = { kind: "label"; message: string } | { kind: "error"; message: string } | { kind: "stopped"; message: string; retry: boolean };
interface TurnFrame { prompt: boolean; pictures: boolean }

// A chat body is one React root. Each native box inside it (a turn's frame,
// its prompt, work rows, reply, cards) holds one portal, drawn from a part.
// Sanitized markup and streamed rows stay native boundaries the history merge
// moves between turns, so a turn keeps a model of which boxes it owns.
type Part =
  | { kind: "frame"; value: TurnFrame; setTurn(element: HTMLDivElement | null): void }
  | { kind: "background"; value: BackgroundQuestions; signature: string }
  | { kind: "menu"; value: RerunView }
  | { kind: "coder"; view: ReactElement }
  | { kind: "route"; callId: string }
  | { kind: "chart"; card: ChartCard; chart: Chart }
  | { kind: "notice"; value: StatusNotice }
  | { kind: "product"; html: string }
  | { kind: "replyPictures"; images: SentImage[] }
  | { kind: "sentFile"; file: SentFile }
  | { kind: "pictures"; images: PicRef[] }
  | { kind: "files"; files: AskFile[] }
  | { kind: "sources"; items: Source[] }
  | { kind: "actions"; value: ReplyActions }
  | { kind: "approval"; value: ApprovalView }
  | { kind: "suggestions"; replies: string[]; send: (text: string) => void }
  | { kind: "prompt"; html: string }
  | { kind: "work"; store: RowStore<WorkView> }
  | { kind: "reply"; store: RowStore<string> };
type Kind = Part["kind"];
type Slot = Part & { key: number };
interface Owner { root: DocumentView; host: HTMLElement | null; body: ChatBody | null; setBody(element: HTMLDivElement | null): void; slots: Map<HTMLElement, Slot>; disposed: boolean; next: number }
/// The boxes of one kind a turn owns. Single kinds keep only the latest box.
interface Boxes { owner: Owner; boxes: HTMLElement[] }
type TurnModel = Map<Kind, Boxes>;

// A retained chat body owns all its portals, including detached turns while
// history is merged. Transfer moved boxes before discarding the fresh turn.
const owners = new WeakMap<ChatBody, Owner>();
const turnModels = new WeakMap<Turn, TurnModel>();
// Updates to a box the user is selecting text in wait until the selection leaves it.
const deferred = new Map<HTMLElement, { apply(): void; resume(): void }>();

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

/// A detached live body stays owned; only cache eviction releases its root.
export function disposeChatBody(body: ChatBody) {
  const owner = owners.get(body);
  if (!owner || owner.disposed) return;

  owner.disposed = true;
  for (const box of owner.slots.keys()) cancelDeferred(box);
  owner.root.unmount();
  owner.slots.clear();
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
    owner.slots.set(host, { kind: "frame", key: ++owner.next, value, setTurn: element => { frame.turn = element; } });
    return frame;
  });
  draw(owner);

  return frames.map(frame => {
    if (!frame.turn) throw new Error("Chat turn did not mount");
    turnModels.set(frame.turn, new Map([["frame", { owner, boxes: [frame.host] }]]));
    return frame.turn;
  });
}

export function createChatTurn(body: ChatBody, value: TurnFrame): Turn {
  const turn = createChatTurns(body, [value])[0];
  if (!turn) throw new Error("Chat turn did not mount");
  return turn;
}

export function chatTurnHost(turn: Turn): HTMLElement {
  return turnModels.get(turn)?.get("frame")?.boxes[0] ?? turn;
}

export function ensureChatPrompt(turn: Turn): HTMLElement | null {
  const entry = turnModels.get(turn)?.get("frame"), host = entry?.boxes[0], frame = host && entry.owner.slots.get(host);
  if (!entry || frame?.kind !== "frame") return null;

  if (!frame.value.prompt) {
    frame.value = { ...frame.value, prompt: true };
    draw(entry.owner);
  }

  return turn.querySelector<HTMLElement>(":scope > .dcoderen.user");
}

export function discardChatWork(turn: Turn) {
  const model = turnModels.get(turn);
  turnModels.delete(turn);
  if (!model) return;

  const touched = new Set<Owner>();
  for (const entry of model.values()) {
    for (const box of entry.boxes) { cancelDeferred(box); entry.owner.slots.delete(box); }
    touched.add(entry.owner);
  }

  for (const owner of touched) draw(owner);
}

export function renderChatBackground(body: ChatBody, box: HTMLElement, value: BackgroundQuestions) {
  const owner = ownerOf(body), prior = owner.slots.get(box), signature = JSON.stringify(value);
  if (prior?.kind === "background" && prior.signature === signature) return false;

  place(owner, box, { kind: "background", value, signature }, true);
  draw(owner);
  return true;
}

export function discardChatBackground(body: ChatBody, box: HTMLElement) {
  const owner = owners.get(body);
  if (owner?.slots.get(box)?.kind === "background" && owner.slots.delete(box)) draw(owner);
}

export function renderChatCoderCards(body: ChatBody, cards: readonly { host: HTMLElement; view: ReactElement }[]) {
  const owner = ownerOf(body), keys = new Map<HTMLElement, number>();
  for (const [box, slot] of owner.slots) if (slot.kind === "coder") { keys.set(box, slot.key); owner.slots.delete(box); }

  for (const card of cards) owner.slots.set(card.host, { kind: "coder", key: keys.get(card.host) ?? ++owner.next, view: card.view });
  draw(owner);
}

export function renderChatPrompt(body: ChatBody, turn: Turn, box: HTMLElement, html: string) {
  show(body, turn, box, { kind: "prompt", html });
}

export function updateChatPrompt(turn: Turn, html: string) {
  const entry = turnModels.get(turn)?.get("prompt"), box = entry?.boxes[0];
  if (!entry || !box || entry.owner.slots.get(box)?.kind !== "prompt") return false;

  whenUnselected(box, true, () => {
    const slot = entry.owner.slots.get(box);
    if (slot?.kind !== "prompt") return;
    entry.owner.slots.set(box, { ...slot, html });
    draw(entry.owner);
  });
  return true;
}

export function renderChatPictures(body: ChatBody, turn: Turn, box: HTMLElement, images: PicRef[]) {
  show(body, turn, box, { kind: "pictures", images });
}

export function renderChatFiles(body: ChatBody, turn: Turn, box: HTMLElement, files: AskFile[]) {
  show(body, turn, box, { kind: "files", files });
}

/// Streaming updates notify only their row; a new row draws the body once.
export function renderChatWork(body: ChatBody, turn: Turn, box: HTMLElement, value: WorkView, after: () => void) {
  const owner = ownerOf(body), prior = owner.slots.get(box);
  track(turn, owner, "work", box);

  whenUnselected(box, prior?.kind === "work", () => {
    if (prior?.kind === "work") prior.store.set(value);
    else { place(owner, box, { kind: "work", store: rowStore(value) }, false); draw(owner); }
    after();
  });
}

export function renderChatReply(body: ChatBody, turn: Turn, box: HTMLElement, html: string) {
  const owner = ownerOf(body), prior = owner.slots.get(box);
  track(turn, owner, "reply", box);
  if (prior?.kind === "reply" && prior.store.get() === html && !deferred.has(box)) return;

  whenUnselected(box, prior?.kind === "reply", () => {
    if (prior?.kind === "reply") prior.store.set(html);
    else { place(owner, box, { kind: "reply", store: rowStore(html) }, false); draw(owner); }
  });
}

export function renderChatApproval(body: ChatBody, turn: Turn, box: HTMLElement, value: ApprovalView) {
  show(body, turn, box, { kind: "approval", value }, true);
}

export function renderChatSuggestions(body: ChatBody, turn: Turn, box: HTMLElement, replies: string[], send: (text: string) => void) {
  show(body, turn, box, { kind: "suggestions", replies, send }, true);
}

export function renderChatNotice(body: ChatBody, turn: Turn, box: HTMLElement, value: StatusNotice) {
  show(body, turn, box, { kind: "notice", value });
}

export function renderChatActions(body: ChatBody, turn: Turn, box: HTMLElement, value: ReplyActions) {
  show(body, turn, box, { kind: "actions", value }, true);
}

export function renderChatRerunMenu(body: ChatBody, turn: Turn, box: HTMLElement, value: RerunView) {
  show(body, turn, box, { kind: "menu", value });
}

export function renderChatSources(body: ChatBody, turn: Turn, box: HTMLElement, items: Source[]) {
  show(body, turn, box, { kind: "sources", items }, true);
}

export function renderChatRoute(body: ChatBody, turn: Turn, box: HTMLElement, callId: string) {
  show(body, turn, box, { kind: "route", callId });
}

export function renderChatChart(body: ChatBody, turn: Turn, box: ChartCard, chart: Chart) {
  show(body, turn, box, { kind: "chart", card: box, chart });
}

export function renderChatProduct(body: ChatBody, turn: Turn, box: HTMLElement, html: string) {
  show(body, turn, box, { kind: "product", html });
}

export function renderChatReplyPictures(body: ChatBody, turn: Turn, box: HTMLElement, images: SentImage[]) {
  show(body, turn, box, { kind: "replyPictures", images: images.slice() }, true);
}

export function renderChatSentFile(body: ChatBody, turn: Turn, box: HTMLElement, file: SentFile) {
  show(body, turn, box, { kind: "sentFile", file });
}

export const discardChatRerunMenu = (turn: Turn) => discard(turn, "menu");
export const discardChatSources = (turn: Turn) => discard(turn, "sources");
export const discardChatActions = (turn: Turn) => discard(turn, "actions");
export const discardChatApproval = (turn: Turn) => discard(turn, "approval");
export const discardChatSuggestions = (turn: Turn) => discard(turn, "suggestions");

export function discardChatNotice(turn: Turn, box: HTMLElement) {
  const entry = turnModels.get(turn)?.get("notice");
  if (!entry) return;

  untrack(turn, "notice", box);
  entry.owner.slots.delete(box);
  draw(entry.owner);
}

// History merges a fresh turn into the shown one. A moved box keeps its
// portal; the shown turn now owns it.
export const transferChatPrompt = (from: Turn, to: Turn) => move(from, to, "prompt");
export const transferChatFiles = (from: Turn, to: Turn) => move(from, to, "files");
export const transferChatPictures = (from: Turn, to: Turn) => move(from, to, "pictures");
export const transferChatReply = (from: Turn, to: Turn) => move(from, to, "reply");
export const transferChatApproval = (from: Turn, to: Turn) => replace(from, to, ["approval"]);
export const transferChatSuggestions = (from: Turn, to: Turn) => replace(from, to, ["suggestions"]);
/// The sent box holds routes, charts, products, pictures and files.
export const transferChatSentFiles = (from: Turn, to: Turn) => replace(from, to, ["route", "chart", "product", "replyPictures", "sentFile"]);

export function transferChatNotice(from: Turn, to: Turn, box: HTMLElement) {
  const entry = turnModels.get(from)?.get("notice");
  if (!entry || !entry.boxes.includes(box)) return;

  untrack(from, "notice", box);
  const target = modelOf(to).get("notice");
  if (target) target.boxes.push(box); else modelOf(to).set("notice", { owner: entry.owner, boxes: [box] });
}

// ---------- Turn model ----------

/// Kinds a turn holds one box of; a new box replaces the old in the model.
const SINGLE = new Set<Kind>(["frame", "menu", "replyPictures", "pictures", "files", "sources", "actions", "approval", "suggestions", "prompt", "work", "reply"]);

/// Shows `part` in `box` and records it on `turn`. `keep` updates a portal
/// already in `box` in place; otherwise the part mounts fresh.
function show(body: ChatBody, turn: Turn, box: HTMLElement, part: Part, keep = false) {
  const owner = ownerOf(body);
  track(turn, owner, part.kind, box);
  place(owner, box, part, keep);
  draw(owner);
}

function place(owner: Owner, box: HTMLElement, part: Part, keep: boolean) {
  const prior = owner.slots.get(box);
  owner.slots.set(box, { ...part, key: keep && prior?.kind === part.kind ? prior.key : ++owner.next });
}

function track(turn: Turn, owner: Owner, kind: Kind, box: HTMLElement) {
  const model = modelOf(turn), entry = model.get(kind);
  if (entry && !SINGLE.has(kind)) entry.boxes.push(box);
  else model.set(kind, { owner, boxes: [box] });
}

function untrack(turn: Turn, kind: Kind, box: HTMLElement) {
  const model = turnModels.get(turn), entry = model?.get(kind);
  if (!entry) return;

  entry.boxes = entry.boxes.filter(item => item !== box);
  if (!entry.boxes.length) model?.delete(kind);
}

function discard(turn: Turn, kind: Kind) {
  const entry = turnModels.get(turn)?.get(kind);
  turnModels.get(turn)?.delete(kind);
  if (!entry) return;

  for (const box of entry.boxes) entry.owner.slots.delete(box);
  draw(entry.owner);
}

function move(from: Turn, to: Turn, kind: Kind) {
  const entry = turnModels.get(from)?.get(kind);
  if (!entry) return;

  turnModels.get(from)?.delete(kind);
  modelOf(to).set(kind, entry);
}

/// Moves `from`'s boxes of `kinds` to `to`, dropping the portals `to` had.
function replace(from: Turn, to: Turn, kinds: readonly Kind[]) {
  const source = turnModels.get(from), target = modelOf(to), touched = new Set<Owner>();
  for (const kind of kinds) {
    const next = source?.get(kind), prior = target.get(kind);
    source?.delete(kind);
    if (prior) { for (const box of prior.boxes) prior.owner.slots.delete(box); touched.add(prior.owner); }
    if (next) { target.set(kind, next); touched.add(next.owner); } else target.delete(kind);
  }

  for (const owner of touched) draw(owner);
}

function modelOf(turn: Turn): TurnModel {
  let model = turnModels.get(turn);
  if (!model) { model = new Map(); turnModels.set(turn, model); }
  return model;
}

/// Runs `apply` now, or once the user's selection leaves `box` when `wait`
/// and it is selected. A later update replaces a waiting one.
function whenUnselected(box: HTMLElement, wait: boolean, apply: () => void) {
  const selected = () => {
    const selection = document.getSelection();
    return !!selection && !selection.isCollapsed && selection.containsNode(box, true);
  };

  if (wait && selected()) {
    const waiting = deferred.get(box);
    if (waiting) { waiting.apply = apply; return; }

    const resume = () => {
      const latest = deferred.get(box);
      if (!latest || selected()) return;
      cancelDeferred(box);
      latest.apply();
    };
    deferred.set(box, { apply, resume });
    document.addEventListener("selectionchange", resume);
    return;
  }

  cancelDeferred(box);
  apply();
}

function cancelDeferred(box: HTMLElement) {
  const waiting = deferred.get(box);
  if (!waiting) return;

  document.removeEventListener("selectionchange", waiting.resume);
  deferred.delete(box);
}

// ---------- Body root ----------

function ownerOf(body: ChatBody) {
  let owner = owners.get(body);
  if (!owner) { owner = makeOwner(null); owners.set(body, owner); }
  return owner;
}

function makeOwner(host: HTMLElement | null): Owner {
  const owner: Owner = { root: createDocumentView(host ?? document.createElement("div")), host, body: null, setBody: element => { owner.body = element; }, slots: new Map(), disposed: false, next: 0 };
  return owner;
}

function draw(owner: Owner) {
  if (owner.disposed) return;

  flushSync(() => owner.root.render(<>
    {owner.host && <div key="body" className="dbody" id="body" ref={owner.setBody} />}
    {[...owner.slots].map(([box, slot]) => createPortal(partView(slot), box, slot.key))}
  </>));
}

function partView(part: Part): ReactNode {
  switch (part.kind) {
    case "frame": return <div className="turn dcoderlog" ref={part.setTurn}>{part.value.prompt && <div className="dcoderen user" key="prompt" />}{part.value.pictures && <div className="dpics" key="pictures" />}<template key="anchor" data-chat-anchor="" /></div>;
    case "background": return <BackgroundView value={part.value} />;
    case "menu": return <RerunMenu value={part.value} />;
    case "coder": return part.view;
    case "route": return <RouteView callId={part.callId} />;
    case "chart": return <ChartView card={part.card} initial={part.chart} />;
    case "notice": return <Notice value={part.value} />;
    case "product": return <Html html={part.html} />;
    case "replyPictures": return <ReplyPictures images={part.images} />;
    case "sentFile": return <SentFileCard file={part.file} />;
    case "pictures": return <PromptPictures images={part.images} />;
    case "files": return <Files files={part.files} />;
    case "sources": return <Sources items={part.items} />;
    case "actions": return <ReplyActionsView value={part.value} />;
    case "approval": return <Approval value={part.value} />;
    case "suggestions": return <Suggestions replies={part.replies} send={part.send} />;
    case "prompt": return <Prompt html={part.html} />;
    case "work": return <WorkSlot store={part.store} />;
    case "reply": return <ReplySlot store={part.store} />;
  }
}

// Streaming updates notify only their row; portal membership remains synchronous.
interface RowStore<T> { get(): T; subscribe(listener: () => void): () => void; set(value: T): void }
function rowStore<T>(initial: T): RowStore<T> {
  let value = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    subscribe: listener => { listeners.add(listener); return () => { listeners.delete(listener); }; },
    set: next => { if (Object.is(value, next)) return; value = next; flushSync(() => { for (const listener of listeners) listener(); }); },
  };
}

const WorkSlot = memo(function WorkSlot({ store }: { store: RowStore<WorkView> }) {
  return <Work value={useSyncExternalStore(store.subscribe, store.get, store.get)} />;
});
const ReplySlot = memo(function ReplySlot({ store }: { store: RowStore<string> }) {
  return <Html html={useSyncExternalStore(store.subscribe, store.get, store.get)} />;
});

// ---------- Parts ----------


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
function RerunMenu({ value }: { value: RerunView }) {
  const [thinking, setThinking] = useState("default");
  return <>{value.choices.map(choice => <button key={choice.id} className="mi" role="menuitem" data-rerun={choice.id} aria-current={choice.used ? "true" : undefined} onClick={event => { event.stopPropagation(); value.run(choice.id, thinking === "default" ? null : thinking); }}><b>{choice.name}</b>{choice.used && <span className="ok" dangerouslySetInnerHTML={{ __html: ICON.check }} />}<small>{choice.detail}</small></button>)}<div className="think">Thinking<span className="dchips">{value.levels.map(level => <button key={level.id} className="dchip" data-rthink={level.id} aria-pressed={level.id === thinking} onClick={event => { event.stopPropagation(); setThinking(level.id); }}>{level.label}</button>)}</span></div></>;
}
const ReplyActionsView = memo(function ReplyActionsView({ value }: { value: ReplyActions }) {
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null), generation = useRef(0);
  useEffect(() => () => { generation.current++; if (timer.current) clearTimeout(timer.current); }, []);
  const copyReply = async (button: HTMLButtonElement) => {
    const mine = ++generation.current;
    if (timer.current) clearTimeout(timer.current);
    const text = button.closest(".turn")?.querySelector<HTMLElement>(":scope > .dcoderen.agent")?.innerText.trim() ?? "";
    try { await writeClipboard(text); if (generation.current === mine) setCopyState("copied"); }
    catch { if (generation.current === mine) setCopyState("failed"); }
    if (generation.current === mine) timer.current = setTimeout(() => setCopyState("idle"), 1200);
  };
  const label = copyState === "copied" ? "Copied" : copyState === "failed" ? "Copy failed" : "Copy";
  return <>{value.copy && <button className="dcopy" data-act="copyReply" onClick={event => { event.stopPropagation(); void copyReply(event.currentTarget); }}><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: copyState === "copied" ? ICON.check : COPY_ICON }} />{label}</button>}{value.rerun && <span className="drerun"><button className="dcopy" data-act="rerunMenu" aria-haspopup="menu" aria-expanded={false}><span style={{ display: "contents" }}><Icon name="playSmall" /></span>Rerun<span className="dchev" dangerouslySetInnerHTML={{ __html: ICON.chev }} /></button></span>}{value.agent && <span className="dagent" title="The agent that answered">{value.agent}</span>}</>;
});
const Approval = memo(function Approval({ value }: { value: ApprovalView }) {
  return <><div className="head"><AppLogo app={value.app} /><div className="dcol"><b>{value.action}?</b><small>{value.app} asks first</small></div></div>
    {!!value.fields.length && <div className="kv">{value.fields.map((field, i) => <Fragment key={i}><span>{field.label}</span><span>{field.value}</span></Fragment>)}</div>}
    <div className="acts"><button className="dbtn pri" data-d="allow" onClick={() => value.answer("allow")}>Allow<kbd>⌘<span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.ret }} /></kbd></button>{!value.grant && <><button className="dbtn" data-d="session" title="Allow this action without asking until this chat ends" onClick={() => value.answer("session")}>Allow for this session<kbd>⇧⌘<span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.ret }} /></kbd></button><button className="dbtn" data-d="always" title="Allow this action without asking until this chat ends" onClick={() => value.answer("always")}>Always<kbd>⌥⌘<span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.ret }} /></kbd></button></>}<button className="dbtn warn" data-d="deny" onClick={() => value.answer("deny")}>Deny<kbd>⌘D</kbd></button></div></>;
});
const Suggestions = memo(function Suggestions({ replies, send }: { replies: string[]; send: (text: string) => void }) {
  return <>{replies.map((reply, i) => <button className="dbtn" key={`${i}:${reply}`} onClick={() => send(reply)}>{reply}</button>)}</>;
});
const WorkStepRow = memo(function WorkStepRow({ step }: { step: WorkStep }) {
  return <div className="dcoderen tool row"><span className="ki" dangerouslySetInnerHTML={{ __html: KIND_ICONS[step.kind] }} /><span className="vb">{step.verb}</span><code className="tt" title={step.label}>{step.target}</code><span className={`rm${step.state === "bad" ? " bad" : ""}`}>{step.state === "bad" ? "failed" : ""}</span><span className="mk">{step.state === "run" ? <span className="dspin" title="Running" /> : step.state === "stop" ? <span className="ddot off" title="Did not finish" /> : <span className={step.state === "ok" ? "dok" : "dbad"} title={step.state === "ok" ? "Done" : "Failed"} dangerouslySetInnerHTML={{ __html: step.state === "ok" ? ICON.check : ICON.x }} />}</span></div>;
});
function Thought({ value, onToggle }: { value: WorkThought; onToggle(open: boolean): void }) {
  const box = useRef<HTMLDivElement>(null), ended = useRef<HTMLDivElement>(null);
  const [folded, setFolded] = useState(false);
  // A thought that just ended is laid out open once, then folds, so the fold animates.
  useLayoutEffect(() => {
    if (!value.folding) { setFolded(false); return; }
    if (ended.current) ended.current.scrollTop = ended.current.scrollHeight;
    void box.current?.offsetHeight;
    setFolded(true);
  }, [value.folding]);
  const open = value.open && !(value.folding && folded);
  const text = value.live || value.folding ? liveText(value.text) : value.text;
  return <div ref={box} className={`dthink${value.live ? " live" : ""}${open ? " open" : ""}`} data-folding={value.folding ? "" : undefined}><button data-thought={value.key} aria-expanded={open} onClick={() => onToggle(!open)}><span className="lb">{thoughtLabel(value.ms)}</span><span className="cv" dangerouslySetInnerHTML={{ __html: ICON.chev }} /></button><div className="bd"><div>{value.live || value.folding ? <div className="tx"><div ref={ended} className={`kk${value.folding ? " ended" : ""}`} data-kk={value.live ? value.key : undefined}>{text}</div></div> : <div className="tx md">{text.split("\n").map((line, index) => <Fragment key={index}>{index > 0 && <br />}{line}</Fragment>)}</div>}</div></div></div>;
}
function LiveLine({ value }: { value: WorkLive }) {
  const lead = value.thinking ? "Thinking" : "Working";
  return <div className="dthink dlive live"><span className="lb" title={lead}>{lead}</span>{value.start !== null && <span className="since" data-since={value.start}>{duration(Date.now() - value.start)}</span>}</div>;
}
const Work = memo(function Work({ value }: { value: WorkView }) {
  return <>{value.heading && <AgentTurnHeading value={value.heading} onToggle={value.toggleSteps} />}{value.thought && <Thought value={value.thought} onToggle={value.toggleThought} />}{value.steps.map(step => <WorkStepRow key={step.key} step={step} />)}{value.live && <LiveLine value={value.live} />}</>;
});
