import { useLayoutEffect, useRef, useState } from "react";
import { fileSize, openViewer } from "./media.tsx";
import { actualView, fitView, pinchStep, zoomAt, type Touches } from "../../stores/agentpane/media-state.ts";
import { FileActions } from "./media-view.tsx";
import type { MediaTab } from "../../stores/agentpane/pane-state.ts";
import { Button } from "../../ui/kit.tsx";

interface ImageControls { loaded(): void; fit(): void; actual(): void }

export function ImageView({ tab, machine }: { tab: MediaTab; machine: string | null }) {
  const stage = useRef<HTMLDivElement>(null), image = useRef<HTMLImageElement>(null);
  const controls = useRef<ImageControls | null>(null);
  const [display, setDisplay] = useState({ zoom: "", fit: true, actual: false });
  const [meta, setMeta] = useState("");
  // Zooming or dragging, the picture's transform changes each frame: the stage says so.
  const [moving, setMoving] = useState({ zooming: false, drag: false });

  useLayoutEffect(() => {
    const box = stage.current, img = image.current;
    if (!box || !img) return;
    let frame = 0, still: ReturnType<typeof setTimeout> | null = null;
    let pinch = 1, drag: { x: number; y: number; pointer: number } | null = null;
    const touches: Touches = new Map();
    const apply = () => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        const current = tab.image;
        if (!current) return;
        img.style.setProperty("--x", `${current.view.x}px`);
        img.style.setProperty("--y", `${current.view.y}px`);
        img.style.setProperty("--scale", `${current.view.scale}`);
        const next = { zoom: `${Math.round(current.view.scale * 100)}%`, fit: current.view.fit, actual: !current.view.fit && current.view.scale === 1 };
        setDisplay(previous => previous.zoom === next.zoom && previous.fit === next.fit && previous.actual === next.actual ? previous : next);
      });
    };
    const fit = () => { if (tab.image) { tab.image.view = fitView(tab.image.w, tab.image.h, box.clientWidth, box.clientHeight); apply(); } };
    const actual = () => { if (tab.image) { tab.image.view = actualView(tab.image.w, tab.image.h, box.clientWidth, box.clientHeight); apply(); } };
    const loaded = () => {
      setMeta(`${img.naturalWidth}×${img.naturalHeight}${tab.size ? ` · ${fileSize(tab.size)}` : ""}`);
      // Moved to the other dock, the picture keeps its zoom.
      if (tab.image && tab.image.w === img.naturalWidth && tab.image.h === img.naturalHeight) { if (tab.image.view.fit) fit(); else apply(); return; }
      tab.image = { view: { scale: 1, x: 0, y: 0, fit: true }, w: img.naturalWidth, h: img.naturalHeight };
      fit();
    };
    const zoom = (factor: number, x: number, y: number) => {
      if (!tab.image) return;
      setMoving(now => now.zooming ? now : { ...now, zooming: true });
      if (still !== null) clearTimeout(still);
      still = setTimeout(() => { still = null; setMoving(now => ({ ...now, zooming: false })); }, 300);
      const rect = box.getBoundingClientRect();
      tab.image.view = zoomAt(tab.image.view, factor, x - rect.left, y - rect.top);
      apply();
    };
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      if (!tab.image) return;
      if (event.ctrlKey) zoom(Math.exp(-event.deltaY * 0.01), event.clientX, event.clientY);
      else {
        const dx = event.deltaX * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? box.clientWidth : 1);
        const dy = event.deltaY * (event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? box.clientHeight : 1);
        tab.image.view = { ...tab.image.view, x: tab.image.view.x - dx, y: tab.image.view.y - dy, fit: false };
        apply();
      }
    };
    const gestureStart = (event: Event) => { event.preventDefault(); pinch = 1; };
    const gestureChange = (event: Event) => {
      event.preventDefault();
      // Two fingers on a phone are the pointer pinch's.
      if (touches.size > 0) return;
      const gesture = event as Event & { scale: number; clientX: number; clientY: number };
      zoom(gesture.scale / pinch, gesture.clientX, gesture.clientY); pinch = gesture.scale;
    };
    const move = (event: PointerEvent) => {
      const step = pinchStep(touches, event.pointerId, event.clientX, event.clientY);
      if (step && tab.image) {
        tab.image.view = { ...tab.image.view, x: tab.image.view.x + step.dx, y: tab.image.view.y + step.dy, fit: false };
        zoom(step.factor, step.x, step.y);
        if (drag?.pointer === event.pointerId) drag = { ...drag, x: event.clientX, y: event.clientY };
        return;
      }

      if (!drag || !tab.image || touches.size > 1) return;
      tab.image.view = { ...tab.image.view, x: tab.image.view.x + event.clientX - drag.x, y: tab.image.view.y + event.clientY - drag.y, fit: false };
      drag = { x: event.clientX, y: event.clientY, pointer: drag.pointer }; apply();
    };
    const up = (event: PointerEvent) => {
      touches.delete(event.pointerId);
      if (drag?.pointer !== event.pointerId) return;
      drag = null; setMoving(now => ({ ...now, drag: false }));
    };
    const down = (event: PointerEvent) => {
      if (event.button !== 0 || !tab.image) return;
      if (event.pointerType === "touch") touches.set(event.pointerId, { x: event.clientX, y: event.clientY });
      if (drag) return;
      box.setPointerCapture(event.pointerId); setMoving(now => ({ ...now, drag: true }));
      drag = { x: event.clientX, y: event.clientY, pointer: event.pointerId };
    };
    const double = () => { if (tab.image?.view.fit) actual(); else fit(); };
    const observer = new ResizeObserver(() => { if (tab.image?.view.fit) fit(); });
    observer.observe(box);
    box.addEventListener("wheel", wheel, { passive: false });
    box.addEventListener("gesturestart", gestureStart); box.addEventListener("gesturechange", gestureChange);
    box.addEventListener("pointerdown", down); box.addEventListener("pointermove", move);
    box.addEventListener("pointerup", up); box.addEventListener("pointercancel", up); box.addEventListener("dblclick", double);
    controls.current = { loaded, fit, actual };
    return () => {
      controls.current = null; observer.disconnect();
      if (drag && box.hasPointerCapture(drag.pointer)) box.releasePointerCapture(drag.pointer);
      if (frame) cancelAnimationFrame(frame);
      if (still !== null) clearTimeout(still);
      box.removeEventListener("wheel", wheel); box.removeEventListener("gesturestart", gestureStart); box.removeEventListener("gesturechange", gestureChange);
      box.removeEventListener("pointerdown", down); box.removeEventListener("pointermove", move);
      box.removeEventListener("pointerup", up); box.removeEventListener("pointercancel", up); box.removeEventListener("dblclick", double);
    };
  }, [tab]);

  return <><div className={`agpstage img${moving.drag ? " drag" : ""}${moving.zooming ? " zooming" : ""}`} ref={stage}><img ref={image} alt={tab.name} draggable={false} src={tab.url ?? undefined} onLoad={() => controls.current?.loaded()} /></div>
    <div className="agpacts"><Button className={display.fit ? "on" : ""} data-fit="1" onClick={() => controls.current?.fit()}>Fit</Button><Button className={display.actual ? "on" : ""} data-fit="0" onClick={() => controls.current?.actual()}>Actual size</Button><span className="zoom">{display.zoom}</span><span className="meta">{tab.note ?? meta}</span><span className="sp" /><span className="hint">Pinch or Ctrl + scroll to zoom, swipe with two fingers or drag to pan</span>{tab.path || tab.exported ? <FileActions tab={tab} machine={machine} label="Open in Preview" /> : <Button data-full="1" title="Full size, to save or copy" onClick={() => { if (tab.url) openViewer([{ url: tab.url, name: tab.name, caption: "", saves: tab.url.startsWith("data:") }], 0); }}>Open full size</Button>}</div></>;
}
