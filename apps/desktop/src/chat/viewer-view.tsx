import { useLayoutEffect, useRef, useState } from "react";
import { commandKey } from "../ui/platform.ts";
import { hintModel } from "../bar/hints.ts";
import { HintView } from "../bar/hints-view.tsx";
import { useBuildCapabilities } from "../ui/tauri.ts";
import { COPY_ICON, ICON } from "../ui/icons.tsx";
import type { ViewerAct, ViewerFile, ViewerPic } from "./media.tsx";
import { useDialogRef } from "../ui/parts.tsx";
import { pinchStep, zoomAt, type ImageView, type Touches } from "../agentpane/media-state.ts";
import { Button, Spinner } from "../ui/kit.tsx";

function ViewerImage({ picture }: { picture: ViewerPic }) {
  const [gone, setGone] = useState(false);
  const zoom = usePinchZoom();
  return gone ? <div className="dmuted dcenter dfill">Picture no longer available</div> : <img ref={zoom} alt={picture.caption ?? ""} src={picture.url} onError={() => setGone(true)} />;
}

/// The host of `url`, or `url` when it has none.
function hostOf(url: string) {
  try { return new URL(url).host; } catch { return url; }
}

/// Two fingers zoom the picture about their midpoint and one pans it while
/// zoomed; a double tap goes back to fitted. Never smaller than fitted.
function usePinchZoom() {
  const image = useRef<HTMLImageElement>(null);

  useLayoutEffect(() => {
    const img = image.current;
    if (!img) return;
    const touches: Touches = new Map();
    let view: ImageView = { scale: 1, x: 0, y: 0, fit: true };
    const show = (next: ImageView) => {
      view = next.scale <= 1 ? { scale: 1, x: 0, y: 0, fit: true } : next;
      img.style.transform = view.fit ? "" : `translate(${view.x}px, ${view.y}px) scale(${view.scale})`;
    };

    const down = (event: PointerEvent) => {
      touches.set(event.pointerId, { x: event.clientX, y: event.clientY });
      img.setPointerCapture(event.pointerId);
    };
    const move = (event: PointerEvent) => {
      const last = touches.get(event.pointerId);
      if (!last) return;

      // The picture's place before the transform, which moves its box by `x`, `y`.
      const rect = img.getBoundingClientRect(), left = rect.left - view.x, top = rect.top - view.y;
      const step = pinchStep(touches, event.pointerId, event.clientX, event.clientY);

      if (step) show(zoomAt({ ...view, x: view.x + step.dx, y: view.y + step.dy }, step.factor, step.x - left, step.y - top));
      else if (touches.size === 1 && !view.fit) show({ ...view, x: view.x + event.clientX - last.x, y: view.y + event.clientY - last.y });
    };
    const up = (event: PointerEvent) => { touches.delete(event.pointerId); };
    const double = () => show({ scale: 1, x: 0, y: 0, fit: true });

    img.addEventListener("pointerdown", down); img.addEventListener("pointermove", move);
    img.addEventListener("pointerup", up); img.addEventListener("pointercancel", up); img.addEventListener("dblclick", double);
    return () => {
      img.removeEventListener("pointerdown", down); img.removeEventListener("pointermove", move);
      img.removeEventListener("pointerup", up); img.removeEventListener("pointercancel", up); img.removeEventListener("dblclick", double);
    };
  }, []);

  return image;
}

export function Viewer({ pictures, at, act, onKey }: { pictures: ViewerPic[]; at: number; act: (a: ViewerAct) => void; onKey: (e: KeyboardEvent, cmd: boolean) => void }) {
  const capabilities = useBuildCapabilities();
  const picture = pictures[at], many = pictures.length > 1;
  const close = useRef<HTMLButtonElement>(null);
  useLayoutEffect(() => { close.current?.focus(); }, [at]);
  const name = picture.name || hostOf(picture.url);
  const about = many ? `${at + 1} of ${pictures.length}` : "";
  const keeps = !!picture.saves, dialog = useDialogRef();
  const keys: [string, string][] = [];
  if (many) keys.push(["arrows", "move"]);
  if (keeps) keys.push(["⌘S", "save"]);
  if (keeps && capabilities.image_clipboard) keys.push(["⌘C", "copy"]);
  keys.push(["esc", "close"]);
  return <div ref={dialog} className="dover dviewer" id="viewer" role="dialog" aria-modal="true" aria-label={name} onKeyDown={event => {
    event.stopPropagation();
    if (event.key === "Tab") {
      const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")], index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      event.preventDefault(); buttons[(index + (event.shiftKey ? -1 : 1) + buttons.length) % buttons.length]?.focus();
    } else onKey(event.nativeEvent, commandKey(event.nativeEvent));
  }}>
    <div className="vt"><span className="tt"><b>{name}</b><small>{about}</small></span>{keeps ? <>{capabilities.image_clipboard && <Button data-v="copy" onClick={() => act("copy")}><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: COPY_ICON }} />Copy<kbd>⌘C</kbd></Button>}<Button look="primary" data-v="save" onClick={() => act("save")}><span style={{ display: "contents" }} dangerouslySetInnerHTML={{ __html: ICON.save }} />Save<kbd>⌘S</kbd></Button></> : !picture.url.startsWith("data:") && <Button data-v="open" onClick={() => act("open")}>Open in browser</Button>}<Button ref={close}  data-v="close" aria-label="Close" onClick={() => act("close")} dangerouslySetInnerHTML={{ __html: ICON.x }} /></div>
    <div className="vi">{many && <Button look="plain" className="nav prev" data-v="prev" aria-label="Previous" disabled={at === 0} onClick={() => act("prev")} dangerouslySetInnerHTML={{ __html: ICON.chev }} />}<ViewerImage key={at} picture={picture} />{many && <Button look="plain" className="nav" data-v="next" aria-label="Next" disabled={at === pictures.length - 1} onClick={() => act("next")} dangerouslySetInnerHTML={{ __html: ICON.chev }} />}</div>
    <div className="cap">{picture.caption}</div><HintView value={hintModel(keys)} />
  </div>;
}

/// A file from a machine over the window: a picture, a player, its text, or
/// a note to open it in this computer's app.
export function FileViewer({ file, act, open }: { file: ViewerFile; act: (a: ViewerAct) => void; open: (file: ViewerFile) => void }) {
  const close = useRef<HTMLButtonElement>(null), dialog = useDialogRef();
  useLayoutEffect(() => { close.current?.focus(); }, []);
  const { read } = file;
  const body = read.kind === "loading" ? <Spinner label={`Reading ${file.name}`} />
    : read.kind === "failed" ? <div className="dmuted dcenter dfill">{read.error}</div>
    : read.kind === "elsewhere" ? <div className="dmuted dcenter dfill">No preview for this file. Open it in its app.</div>
    : read.kind === "text" ? <pre>{read.text}</pre>
    : read.media === "image" ? <ViewerImage picture={{ url: read.url, caption: file.name }} />
    : read.media === "video" ? <video controls autoPlay playsInline src={read.url} />
    : <audio controls autoPlay src={read.url} />;

  return <div ref={dialog} className="dover dviewer dfile" id="viewer" role="dialog" aria-modal="true" aria-label={file.name} onKeyDown={event => {
    event.stopPropagation();
    if (event.key === "Escape") { event.preventDefault(); act("close"); }
  }}>
    <div className="vt"><span className="tt"><b>{file.name}</b><small>{read.kind === "text" && read.cut ? `${file.path}, the start of it` : file.path}</small></span><Button data-v="open" onClick={() => open(file)}>Open</Button><Button ref={close} data-v="close" aria-label="Close" onClick={() => act("close")} dangerouslySetInnerHTML={{ __html: ICON.x }} /></div>
    <div className="vi">{body}</div><HintView value={hintModel([["esc", "close"]])} />
  </div>;
}
