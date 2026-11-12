import { AppLogo } from "./applogo-view.tsx";
import { BackgroundView } from "./background-view.tsx";
import { CoderCard } from "./codercard-view.tsx";
import { Fragment, memo, useEffect, useLayoutEffect, useReducer, useRef, useState, type RefObject } from "react";
import { useStore, type StoreApi } from "zustand";
import { COPY_ICON, ICON, Icon } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";
import { writeClipboard } from "../ui/parts.ts";
import { liveText } from "../agents/karaoke.ts";
import { thoughtLabel } from "../agents/agentturns.ts";
import { KIND_ICONS } from "../agents/agentturns.ts";
import { attachLive } from "../agents/thoughtlive.ts";
import { AgentTurnHeading, Since, StepMarkView, tickClocks } from "../agents/turn-heading-view.tsx";
import { RouteView } from "./route-view.tsx";
import { ChartView } from "./chart-view.tsx";
import { fileKind, fileSize, GONE, GONE_PIC, loadPic, openViewer, pictures, savePic, type PicRef, type ProductCard, type SentFile, type SentImage } from "./media.ts";
import { VOICE_ICON } from "../bar/live.ts";
import type { AskFile } from "./attach.ts";
import { placeReplyElement, placeTurnElement, type Turn, type ApprovalView, type ReplyActions, type RerunView, type Sent, type SentItem, type Source, type StoppedView, type TranscriptState, type TurnView, type WorkLive, type WorkStep, type WorkThought, type WorkView } from "./transcript.ts";
import { cardKey } from "./agentcard.ts";
import { savesName } from "../ui/platform.ts";

// A chat's transcript as React draws it: one list of turns, each drawn from
// its view in the transcript store, then the background work waiting on the
// chat. A turn draws again only when its view changes.

export function TranscriptView({ store }: { store: StoreApi<TranscriptState> }) {
  const { turns, background, shown, top } = useStore(store);
  return <>{top}{turns.map(view => <TurnRow key={view.turn.key} view={view} shown={shown} />)}{background && <div className="dworkq dgrid8"><BackgroundView value={background} /></div>}</>;
}

const TurnRow = memo(function TurnRow({ view, shown }: { view: TurnView; shown: number }) {
  const { turn } = view;
  return <div className="turn dcoderlog" data-at={view.at ?? undefined} ref={element => placeTurnElement(turn, element)}>
    {view.system !== null && <div className="dsystem">{view.system}</div>}
    {view.prompt !== null && <Prompt html={view.prompt} />}
    {view.ranOn !== null && <div className="drerunof dranon">{view.ranOn}</div>}
    {view.pictures && <div className="dpics"><PromptPictures images={view.pictures} /></div>}
    {view.files && <div className="dachips"><Files files={view.files} /></div>}
    {view.rerunOf !== null && <div className="drerunof">{view.rerunOf}</div>}
    {view.work && <WorkBox value={view.work} />}
    {view.coders.map(report => <CoderCard key={cardKey(report)} report={report} shown={shown} />)}
    {view.reply !== null && <Reply turn={turn} html={view.reply} />}
    {view.sent && <SentBox sent={view.sent} />}
    {view.sources.length > 0 && <div className="dsources" aria-label="Sources"><Sources items={view.sources} /></div>}
    {view.approval && <div className="dcard"><Approval value={view.approval} /></div>}
    {view.answered !== null && <div className="dmuted">{view.answered}</div>}
    {view.error !== null && <div className="derrcard"><b>The turn did not finish</b><span>{view.error}</span></div>}
    {view.stopped && <Stopped value={view.stopped} />}
    {view.actions && <div className="dreplyacts"><ReplyActionsView value={view.actions} menu={view.menu !== null} /></div>}
    {view.menu && <div className="dmenu" role="menu" aria-label="Rerun on"><RerunMenu value={view.menu} /></div>}
    {view.replies && <div className="dreplies"><Suggestions replies={view.replies.list} send={view.replies.send} /></div>}
  </div>;
});

/// What a box shows while the user selects text in it: the value from
/// before, until the selection leaves. Without a selection it is `value`,
/// drawn in the same render.
export function useHeld<T>(value: T, box: RefObject<HTMLElement | null>, same: (a: T, b: T) => boolean = Object.is): T {
  const shown = useRef(value);
  const [, resume] = useReducer((n: number) => n + 1, 0);
  const held = !same(shown.current, value) && selecting(box.current);
  const result = held ? shown.current : value;

  useLayoutEffect(() => {
    shown.current = result;
    if (!held) return;
    const check = () => { if (!selecting(box.current)) resume(); };
    document.addEventListener("selectionchange", check);
    return () => document.removeEventListener("selectionchange", check);
  }, [result, held]);

  return result;
}

/// Whether the user is selecting text in `node`.
function selecting(node: HTMLElement | null) {
  const selection = document.getSelection();
  return !!node && !!selection && !selection.isCollapsed && selection.containsNode(node, true);
}

function Prompt({ html }: { html: string }) {
  const box = useRef<HTMLDivElement>(null), shown = useHeld(html, box);
  return <div className="dcoderen user" ref={box}><b>You</b><span dangerouslySetInnerHTML={{ __html: shown }} /></div>;
}

/// The answer's sanitized markup. It is set only when it changes, so
/// pictures filled in it stay; a streaming answer is written in by
/// `streamReply` without drawing the turn again.
function Reply({ turn, html }: { turn: Turn; html: string }) {
  const box = useRef<HTMLSpanElement>(null);

  useLayoutEffect(() => {
    if (!box.current) return;
    showHtml(box.current, html);
    placeReplyElement(turn, box.current);
  }, [turn, html]);

  return <div className="dcoderen agent md"><span style={{ display: "contents" }} ref={box} /></div>;
}

const written = new WeakMap<HTMLElement, string>(), waiting = new WeakMap<HTMLElement, string>();

/// Puts sanitized `html` in `el`, unless it shows it already. While the
/// user selects in `el` the newest waits until the selection leaves.
export function showHtml(el: HTMLElement, html: string) {
  if (!selecting(el)) {
    waiting.delete(el);
    if (written.get(el) !== html) { el.innerHTML = html; written.set(el, html); }
    return;
  }

  if (!waiting.has(el)) {
    const resume = () => {
      const next = waiting.get(el);
      if (next === undefined || !el.isConnected) { waiting.delete(el); document.removeEventListener("selectionchange", resume); return; }
      if (selecting(el)) return;
      document.removeEventListener("selectionchange", resume);
      showHtml(el, next);
    };
    document.addEventListener("selectionchange", resume);
  }

  waiting.set(el, html);
}

function WorkBox({ value }: { value: WorkView }) {
  const box = useRef<HTMLDivElement>(null), shown = useHeld(value, box);
  useLayoutEffect(() => {
    if (box.current) attachLive(box.current, shown.touched);
    tickClocks();
  }, [shown]);
  return <div className="tools dcoderlog" data-key={shown.key} ref={box}><Work value={shown} /></div>;
}

function SentBox({ sent }: { sent: Sent }) {
  return <div className="files dgrid6">{sent.pictures.length > 0 && <div className="drpics"><ReplyPictures images={sent.pictures} /></div>}{sent.items.map((item, index) => <SentPart key={`${item.kind}:${index}`} item={item} />)}</div>;
}

function SentPart({ item }: { item: SentItem }) {
  switch (item.kind) {
    case "products": return item.cards.kind === "products" ? <ProductCards cards={item.cards.cards} /> : <Html html={item.cards.html} />;
    case "file": return <SentFileCard file={item.file} />;
    case "chart": return <div className="dchart"><ChartView initial={item.chart} /></div>;
    case "route": return <div className="dchart droute"><RouteView callId={item.callId} /></div>;
  }
}

function Stopped({ value }: { value: StoppedView }) {
  return <div className="dmuted dstopped">{value.message}{value.retry && <button className="dbtn" data-act="retry" onClick={event => { event.stopPropagation(); value.retry?.(); }}>Retry</button>}</div>;
}

// ---------- Parts ----------

const Html = memo(function Html({ html }: { html: string }) {
  return <span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: html }} />;
});
/// A product's picture over its placeholder once loaded; a broken one
/// leaves the placeholder.
function ProductImage({ url }: { url: string }) {
  const [state, setState] = useState<"loading" | "shown" | "broken">("loading");
  return <span className="dprod-img">{state !== "broken" && <img src={url} alt="" loading="lazy" className={state === "shown" ? "on" : undefined} onLoad={() => setState("shown")} onError={() => setState("broken")} />}</span>;
}
/// Copies a product's link, then shows a check for a moment.
function CopyLink({ url }: { url: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 1200);
    return () => clearTimeout(timer);
  }, [copied]);
  return <button className="dprod-copy" aria-label="Copy link" onClick={event => { event.preventDefault(); event.stopPropagation(); void writeClipboard(url).then(() => setCopied(true), error => console.warn(error)); }}>{copied ? <Icon name="check" /> : <Icon name="copy" />}</button>;
}
/// The same markup as a reply's ```product blocks, which Rust writes.
const ProductCards = memo(function ProductCards({ cards }: { cards: ProductCard[] }) {
  return <div className="dprods">{cards.map((card, index) => <div className="dprod" key={index}>
    <a className="dprod-card" href={card.url} aria-label={card.label}>
      {card.image !== null && <ProductImage url={card.image} />}
      <span className="dprod-info"><span className="dprod-store">{card.store}</span><span className="dprod-title">{card.title}</span>
        {card.price !== null ? <span className="dprod-price"><b>{card.price}</b>{card.was !== null && <s>{card.was}</s>}</span> : <span className="dprod-meta">See price on the site</span>}
        {card.meta !== null && <span className="dprod-meta">{card.meta}</span>}</span>
    </a>
    <CopyLink url={card.url} />
  </div>)}</div>;
});
function ReplyPicture({ image, images, index }: { image: SentImage; images: readonly SentImage[]; index: number }) {
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
  return <span className={`drpic${waiting ? " wait" : ""}`}><button className="pic" aria-label={image.caption || name || "Picture"} onClick={() => openViewer([...images], index)}>{src && <img alt="" src={src} onLoad={() => setWaiting(false)} onError={() => setGone(true)} />}</button>{file && <button className="save" aria-label={`Save ${name}`} title={`Save to ${savesName()}`} onClick={() => void savePic(file)} dangerouslySetInnerHTML={{ __html: ICON.save }} />}</span>;
}
const ReplyPictures = memo(function ReplyPictures({ images }: { images: readonly SentImage[] }) {
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
const PromptPictures = memo(function PromptPictures({ images }: { images: readonly PicRef[] }) {
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
const Files = memo(function Files({ files }: { files: readonly AskFile[] }) {
  return <>{files.map((file, i) => <FileChip key={i} file={file} />)}</>;
});
const Sources = memo(function Sources({ items }: { items: readonly Source[] }) {
  const [open, setOpen] = useState(false);
  const shown = open ? items : items.slice(0, 3), more = items.length - shown.length;
  return <>{shown.map((source, i) => <button key={`${source.url}:${i}`} className="dsrc" data-i={i} title={source.title || source.url} onClick={() => void invoke("open_url", { url: source.url }).catch(() => {})}><i>{source.site[0].toUpperCase()}</i><span>{source.site}</span></button>)}{more > 0 && <button className="dsrc more" aria-label={`Show ${more} more sources`} onClick={() => setOpen(true)}>+{more}</button>}</>;
});
function RerunMenu({ value }: { value: RerunView }) {
  const [thinking, setThinking] = useState("default");
  return <>{value.choices.map(choice => <button key={choice.id} className="mi" role="menuitem" data-rerun={choice.id} aria-current={choice.used ? "true" : undefined} onClick={event => { event.stopPropagation(); value.run(choice.id, thinking === "default" ? null : thinking); }}><b>{choice.name}</b>{choice.used && <span className="ok" dangerouslySetInnerHTML={{ __html: ICON.check }} />}<small>{choice.detail}</small></button>)}<div className="think">Thinking<span className="dchips">{value.levels.map(level => <button key={level.id} className="dchip" data-rthink={level.id} aria-pressed={level.id === thinking} onClick={event => { event.stopPropagation(); setThinking(level.id); }}>{level.label}</button>)}</span></div></>;
}
const ReplyActionsView = memo(function ReplyActionsView({ value, menu }: { value: ReplyActions; menu: boolean }) {
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
  return <>{value.copy && <button className="dcopy" data-act="copyReply" onClick={event => { event.stopPropagation(); void copyReply(event.currentTarget); }}><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: copyState === "copied" ? ICON.check : COPY_ICON }} />{label}</button>}{value.rerun && <span className="drerun"><button className="dcopy" data-act="rerunMenu" aria-haspopup="menu" aria-expanded={menu} onClick={event => { event.stopPropagation(); value.rerun?.(); }}><span style={{ display: "contents" }}><Icon name="playSmall" /></span>Rerun<span className="dchev" dangerouslySetInnerHTML={{ __html: ICON.chev }} /></button></span>}{value.agent && <span className="dagent" title="The agent that answered">{value.agent}</span>}</>;
});
const Approval = memo(function Approval({ value }: { value: ApprovalView }) {
  return <><div className="head"><AppLogo app={value.app} /><div className="dcol"><b>{value.action}?</b><small>{value.app} asks first</small></div></div>
    {!!value.fields.length && <div className="kv">{value.fields.map((field, i) => <Fragment key={i}><span>{field.label}</span><span>{field.value}</span></Fragment>)}</div>}
    <div className="acts"><button className="dbtn pri" data-d="allow" onClick={() => value.answer("allow")}>Allow<kbd>⌘<span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.ret }} /></kbd></button>{!value.grant && <><button className="dbtn" data-d="session" title="Allow this action without asking until this chat ends" onClick={() => value.answer("session")}>Allow for this session<kbd>⇧⌘<span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.ret }} /></kbd></button><button className="dbtn" data-d="always" title="Allow this action without asking until this chat ends" onClick={() => value.answer("always")}>Always<kbd>⌥⌘<span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.ret }} /></kbd></button></>}<button className="dbtn warn" data-d="deny" onClick={() => value.answer("deny")}>Deny<kbd>⌘D</kbd></button></div></>;
});
const Suggestions = memo(function Suggestions({ replies, send }: { replies: readonly string[]; send: (text: string) => void }) {
  return <>{replies.map((reply, i) => <button className="dbtn" key={`${i}:${reply}`} onClick={() => send(reply)}>{reply}</button>)}</>;
});
const STEP_MARKS = { run: "running", ok: "done", bad: "failed", stop: "stopped" } as const;
const WorkStepRow = memo(function WorkStepRow({ step }: { step: WorkStep }) {
  return <div className="dcoderen tool row"><span className="ki" dangerouslySetInnerHTML={{ __html: KIND_ICONS[step.kind] }} /><span className="vb">{step.verb}</span><code className="tt" title={step.label}>{step.target}</code><span className={`rm${step.state === "bad" ? " bad" : ""}`}>{step.state === "bad" ? "failed" : ""}</span><span className="mk"><StepMarkView mark={STEP_MARKS[step.state]} /></span></div>;
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
  return <div className="dthink dlive live"><span className="lb" title={lead}>{lead}</span>{value.start !== null && <Since className="since" start={value.start} />}</div>;
}
const Work = memo(function Work({ value }: { value: WorkView }) {
  return <>{value.heading && <AgentTurnHeading value={value.heading} onToggle={value.toggleSteps} />}{value.thought && <Thought value={value.thought} onToggle={value.toggleThought} />}{value.steps.map(step => <WorkStepRow key={step.key} step={step} />)}{value.live && <LiveLine value={value.live} />}</>;
});
