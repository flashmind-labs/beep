// The Device tab's view: the picker with the device's state and Boot or
// Shut down, its screen fitted to the view, and its hardware buttons.
import { memo, useEffect, useLayoutEffect, useRef, type ChangeEvent, type FormEvent, type KeyboardEvent, type PointerEvent } from "react";
import { useStore } from "zustand";
import { Icon, type IconName } from "../ui/icons.tsx";
import { deviceModel, pickDevice, power, sendInput, setDeviceShown, setPainter, showAgain } from "./device.ts";
import {
  BUTTON_NAMES, type Device, type DeviceButton, type DeviceMachine, type Rect, type ScreenState, type Size, type TouchPhase,
  buttonsFor, fitRect, keyInput, maxSide, pickedDevice, pickValue, powerChip, readPickValue, touchPoint,
} from "./device-state.ts";

const BUTTON_ICONS: Record<DeviceButton, IconName> = {
  Home: "devHome", Back: "devBack", AppSwitch: "devRecent", Rotate: "devRotate", Power: "devPower", VolumeDown: "devVolumeDown", VolumeUp: "devVolumeUp",
};

export const DevicePane = memo(function DevicePane({ hidden }: { hidden: boolean }) {
  const model = useStore(deviceModel);
  const device = pickedDevice(model.machines, model.pick);
  const machine = model.machines.find(m => m.machine_id === model.pick?.machine) ?? null;
  const live = model.screen.kind === "live";

  useEffect(() => { setDeviceShown(!hidden); }, [hidden]);
  useEffect(() => () => setDeviceShown(false), []);

  const anyDevice = model.machines.some(m => m.devices.length > 0);
  if (model.listed && !anyDevice) {
    const names = model.machines.map(m => m.machine_name).join(" or ");
    return <div className="agpview device" hidden={hidden}><div className="agdevnote">{model.error || `No emulator or simulator on ${names || "the machines"}.`}{model.machines.filter(m => m.error).map(m => <small key={m.machine_id}>{m.machine_name}: {m.error}</small>)}</div></div>;
  }

  return <div className="agpview device" hidden={hidden}>
    <DeviceHead machines={model.machines} device={device} busy={model.busy} />
    <div className="derr" role="status" hidden={!model.error}>{model.error}</div>
    <Screen device={device} />
    <div className="agpacts agdevkeys">
      {device && buttonsFor(device.platform).map(button => <button key={button} type="button" className="dbtn ibtn" title={BUTTON_NAMES[button]} aria-label={BUTTON_NAMES[button]} disabled={!live}
        onClick={() => sendInput({ Button: { button } })}><Icon name={BUTTON_ICONS[button]} /></button>)}
      <span className="sp" />
      {device && machine && <span className="meta">{[device.os, machine.machine_name].filter(Boolean).join(" · ")}</span>}
    </div>
  </div>;
});

function DeviceHead({ machines, device, busy }: { machines: readonly DeviceMachine[]; device: Device | null; busy: boolean }) {
  const pick = useStore(deviceModel, s => s.pick);
  const chip = device ? powerChip(device.state) : null;
  const choose = (event: ChangeEvent<HTMLSelectElement>) => {
    const next = readPickValue(event.currentTarget.value);
    if (next) pickDevice(next);
  };

  return <div className="agdevhead">
    <select className="agdevpick" aria-label="Device" value={pick ? pickValue(pick) : ""} onChange={choose}>
      {!pick && <option value="">Reading devices…</option>}
      {machines.filter(m => m.devices.length > 0).map(m => <optgroup key={m.machine_id} label={m.machine_name}>
        {m.devices.map(d => <option key={d.id} value={pickValue({ machine: m.machine_id, device: d.id })}>{d.name}</option>)}
      </optgroup>)}
    </select>
    {device && chip && <span className={`chip ${chip.cls}`}>{device.state === "Booting" && <span className="dspin" />}{chip.text}</span>}
    <span className="sp" />
    {device && (device.state === "Off"
      ? <button type="button" className="dbtn" disabled={busy} onClick={() => void power(true)}>{busy ? "Booting…" : "Boot"}</button>
      : <button type="button" className="dbtn" disabled={busy} onClick={() => void power(false)}>{busy ? "Shutting down…" : "Shut down"}</button>)}
  </div>;
}

/// The device's screen: frames drawn on a canvas, fitted with their aspect
/// kept. A pointer touches it; with it focused, keys and pastes type into
/// the device through a hidden text field, which also takes composed text.
function Screen({ device }: { device: Device | null }) {
  const screen = useStore(deviceModel, s => s.screen);
  const stage = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const keys = useRef<HTMLTextAreaElement>(null);
  // The frame drawn last, kept to draw again when the view resizes.
  const frame = useRef<ImageBitmap | null>(null);
  // A drag's latest move, sent once per animation frame.
  const move = useRef<{ x: number; y: number } | null>(null);
  const tick = useRef<number | null>(null);
  const touching = useRef(false);

  useLayoutEffect(() => {
    const box = stage.current, el = canvas.current;
    if (!box || !el) return;
    const redraw = () => {
      const ratio = devicePixelRatio || 1;
      el.width = Math.round(box.clientWidth * ratio);
      el.height = Math.round(box.clientHeight * ratio);
      paintFrame(el, frame.current);
    };
    setPainter(bitmap => {
      frame.current?.close();
      frame.current = bitmap;
      paintFrame(el, bitmap);
    }, () => maxSide({ width: box.clientWidth, height: box.clientHeight }, devicePixelRatio || 1));
    const watch = new ResizeObserver(redraw);
    watch.observe(box);
    redraw();
    return () => {
      watch.disconnect();
      setPainter(null, () => 0);
      frame.current?.close();
      frame.current = null;
    };
  }, []);

  // A new view starts blank.
  useEffect(() => {
    if (screen.kind === "live") return;
    frame.current?.close();
    frame.current = null;
    const el = canvas.current;
    el?.getContext("2d")?.clearRect(0, 0, el.width, el.height);
  }, [screen.kind]);

  useEffect(() => () => { if (tick.current !== null) cancelAnimationFrame(tick.current); }, []);

  const live = screen.kind === "live";

  /// Where the frame sits in the canvas, in CSS pixels.
  const drawn = (): Rect | null => {
    const el = canvas.current, bitmap = frame.current;
    if (!el || !bitmap) return null;
    return fitRect({ width: el.clientWidth, height: el.clientHeight }, { width: bitmap.width, height: bitmap.height });
  };
  const at = (event: PointerEvent<HTMLCanvasElement>, clamp: boolean) => {
    const rect = drawn();
    if (!rect) return null;
    const box = event.currentTarget.getBoundingClientRect();
    return touchPoint(rect, event.clientX - box.left, event.clientY - box.top, clamp);
  };
  const touch = (phase: TouchPhase, point: { x: number; y: number }) => sendInput({ Touch: { phase, ...point } });

  const down = (event: PointerEvent<HTMLCanvasElement>) => {
    keys.current?.focus({ preventScroll: true });
    if (!live || event.button !== 0) return;
    const point = at(event, false);
    if (!point) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    touching.current = true;
    touch("Down", point);
  };
  const moved = (event: PointerEvent<HTMLCanvasElement>) => {
    if (!touching.current) return;
    move.current = at(event, true);
    tick.current ??= requestAnimationFrame(() => {
      tick.current = null;
      const point = move.current;
      move.current = null;
      if (point && touching.current) touch("Move", point);
    });
  };
  const up = (event: PointerEvent<HTMLCanvasElement>) => {
    if (!touching.current) return;
    touching.current = false;
    move.current = null;
    const point = at(event, true);
    if (point) touch("Up", point);
  };

  const key = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    const input = keyInput({ key: event.key, metaKey: event.metaKey, ctrlKey: event.ctrlKey, altKey: event.altKey, isComposing: event.nativeEvent.isComposing });
    if (!input) return;
    event.preventDefault();
    event.stopPropagation();
    sendInput(input);
  };
  const typed = (event: FormEvent<HTMLTextAreaElement>) => {
    const native = event.nativeEvent;
    const field = event.currentTarget;
    if (native instanceof InputEvent && native.isComposing) return;
    const text = field.value;
    field.value = "";
    if (text) sendInput({ Text: { text } });
  };

  return <div className="agdevstage" ref={stage}>
    <canvas ref={canvas} className="agdevscreen" hidden={!live} title="Click to tap, drag to swipe; type while it is focused"
      onPointerDown={down} onPointerMove={moved} onPointerUp={up} onPointerCancel={up} onContextMenu={event => event.preventDefault()} />
    <textarea ref={keys} className="agdevtype" aria-label="Type into the device" autoCapitalize="off" autoCorrect="off" spellCheck={false} tabIndex={live ? 0 : -1}
      onKeyDown={key} onInput={typed} onCompositionEnd={typed} onPaste={event => {
        event.preventDefault();
        const text = event.clipboardData.getData("text/plain");
        if (text) sendInput({ Text: { text } });
      }} />
    {!live && <div className="agdevnote">{note(screen, device)}</div>}
  </div>;
}

function note(screen: ScreenState, device: Device | null) {
  // A device shut down ends its view too; being off says why.
  if (device?.state === "Off") return `${device.name} is off.`;
  if (screen.kind === "ended") return <>{screen.reason}<button type="button" className="dbtn" onClick={showAgain}>Show again</button></>;
  if (!device) return null;
  if (device.state === "Booting") return <><span className="dspin" />Booting {device.name}…</>;
  return <><span className="dspin" />Connecting…</>;
}

/// Draws `bitmap` fitted in the canvas, or clears it.
function paintFrame(el: HTMLCanvasElement, bitmap: ImageBitmap | null) {
  const g = el.getContext("2d");
  if (!g) return;
  g.clearRect(0, 0, el.width, el.height);
  if (!bitmap) return;
  const box: Size = { width: el.width, height: el.height };
  const rect = fitRect(box, { width: bitmap.width, height: bitmap.height });
  g.drawImage(bitmap, rect.x, rect.y, rect.width, rect.height);
}
