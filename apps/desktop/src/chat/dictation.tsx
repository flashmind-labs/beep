// The composers' mic button: records from the microphone, streams 16 kHz PCM
// to the app's dictation commands and writes the transcript at the caret as it
// comes. While it listens, a mound of characters at the foot of the field
// rises with the voice and sends each new word into place. Typing plays
// nothing.
import { useEffect, useRef, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import { useStore } from "zustand";
import { Channel, invoke, logError } from "../ui/tauri.ts";
import { APPLE, onPhone } from "../ui/platform.ts";
import { motionAllowed } from "../ui/motion.ts";
import { Icon } from "../ui/icons.tsx";
import { applyTranscript, createDictationSettings, elapsed, HOLD_OPTION_MS, IDLE, isBareOptionDown, nextTurn, parseEvent, pcmChunks, reanchorDraft, Resampler, startDraft, type DictationDraft, type Turn } from "./dictation-state.ts";

/// The shared setting, read once per window.
export const dictationSettings = createDictationSettings(invoke, () => new Channel<unknown>());

interface Props {
  /// The composer's text field.
  field: RefObject<HTMLTextAreaElement | null>;
  /// The composer's box, which the animation and the timer are drawn over.
  host: RefObject<HTMLElement | null>;
}

/// The mic button, shown on desktops with dictation turned on.
export function DictateButton({ field, host }: Props) {
  const settings = useStore(dictationSettings.state, state => state.saved);
  useEffect(() => { if (!onPhone() && dictationSettings.state.getState().saved === null) void dictationSettings.load(); }, []);

  if (onPhone() || !settings?.enabled) return null;
  return <Dictation field={field} host={host} />;
}

/// The microphone while it records.
interface Mic { stream: MediaStream; context: AudioContext; analyser: AnalyserNode; levels: Uint8Array<ArrayBuffer> }

/// One dictation, from the press to its last transcript.
interface Run {
  id: string | null;
  mic: Mic | null;
  draft: DictationDraft;
  writing: boolean;
  /// Started by holding Option/Alt for push-to-talk.
  held: boolean;
  /// Released before `dictation_start` returned its id.
  stopping: boolean;
  /// Audio recorded before the app gave the dictation its id.
  pending: string[];
  /// Chunks go out one after another.
  sending: Promise<unknown>;
  startedAt: number;
}

const ACTIVE = new Set<Turn["phase"]>(["starting", "listening", "thinking"]);

function Dictation({ field, host }: Props) {
  const [turn, setTurn] = useState<Turn>(IDLE);
  const [now, setNow] = useState(0);
  const run = useRef<Run | null>(null);
  const btn = useRef<HTMLButtonElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const glow = useRef(new Glow());

  const finish = (r: Run) => {
    if (run.current === r) run.current = null;
    if (r.mic) closeMic(r.mic);
    r.mic = null;
  };

  const write = (r: Run, committed: string, partial: string) => {
    const node = field.current;
    if (!node) return;

    const applied = applyTranscript(r.draft, node.value, node.selectionStart, node.selectionEnd, committed, partial);
    r.draft = applied.draft;
    if (!applied.write) return;

    const { value, caret, lead, words } = applied.write;
    const box = host.current;
    if (box) for (const { before, word } of words) glow.current.spawn(box, node, lead + before, word);
    r.writing = true;
    try {
      node.value = value;
      node.setSelectionRange(caret, caret);
      node.dispatchEvent(new Event("input", { bubbles: true }));
    } finally {
      r.writing = false;
    }
  };

  const received = (r: Run, message: unknown) => {
    if (run.current !== r) return;
    const event = parseEvent(message);
    if (!event) return;

    if (event.kind === "transcript") write(r, event.committed, event.partial);
    if (event.kind === "done" || event.kind === "error") finish(r);
    if (event.kind === "thinking" && r.mic) { closeMic(r.mic); r.mic = null; }
    setTurn(t => nextTurn(t, event));
  };

  const send = (r: Run, pcm: Int16Array) => {
    for (const chunk of pcmChunks(pcm)) {
      const id = r.id;
      if (id === null) { r.pending.push(chunk); continue; }
      r.sending = r.sending.then(() => invoke("dictation_audio", { id, pcmBase64: chunk })).catch((error: unknown) => logError(`dictation audio: ${String(error)}`));
    }
  };

  const fail = (r: Run, message: string) => {
    if (run.current !== r) return;
    finish(r);
    if (r.id !== null) void invoke("dictation_cancel", { id: r.id }).catch(() => {});
    setTurn({ phase: "error", error: message });
  };

  const start = async (held = false) => {
    const node = field.current;
    if (!node) return;

    node.focus();
    const r: Run = { id: null, mic: null, draft: startDraft(node.value, node.selectionStart, node.selectionEnd), writing: false, held, stopping: false, pending: [], sending: Promise.resolve(), startedAt: performance.now() };
    run.current = r;
    setTurn({ phase: "starting", error: "" });

    let mic: Mic;
    try { mic = await openMic(pcm => send(r, pcm), () => fail(r, "Microphone disconnected.")); }
    catch { fail(r, "Microphone blocked."); return; }
    if (run.current !== r) { closeMic(mic); return; }
    if (r.stopping) closeMic(mic);
    else r.mic = mic;

    const channel = new Channel<unknown>();
    channel.onmessage = message => received(r, message);
    let id: string;
    try { id = await invoke<string>("dictation_start", { model: null, onEvent: channel }); }
    catch (error) { fail(r, String(error)); return; }
    if (run.current !== r) { void invoke("dictation_cancel", { id }).catch(() => {}); return; }

    r.id = id;
    r.startedAt = performance.now();
    for (const chunk of r.pending.splice(0)) r.sending = r.sending.then(() => invoke("dictation_audio", { id, pcmBase64: chunk })).catch(() => {});
    if (r.stopping) {
      void r.sending.then(() => invoke("dictation_stop", { id })).catch((error: unknown) => fail(r, String(error)));
      return;
    }
    node.focus();
  };

  const stop = () => {
    const r = run.current;
    if (!r) return;
    r.held = false;
    const hadMic = r.mic !== null || r.pending.length > 0;
    if (r.mic) closeMic(r.mic);
    r.mic = null;
    if (r.id === null) {
      if (!hadMic) { cancel(); return; }
      r.stopping = true;
      setTurn(t => nextTurn(t, { kind: "thinking" }));
      return;
    }

    const id = r.id;
    setTurn(t => nextTurn(t, { kind: "thinking" }));
    void r.sending.then(() => invoke("dictation_stop", { id })).catch((error: unknown) => fail(r, String(error)));
  };

  const cancel = () => {
    const r = run.current;
    if (r) {
      finish(r);
      if (r.id !== null) void invoke("dictation_cancel", { id: r.id }).catch(() => {});
    }
    setTurn(IDLE);
  };

  const handlers = useRef({ turn, start, stop, cancel });
  handlers.current = { turn, start, stop, cancel };

  // Holding Option/Alt alone while a composer is on screen runs push-to-talk;
  // another key or click leaves Option shortcuts and character input alone.
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const clearHold = () => { if (timer !== null) { clearTimeout(timer); timer = null; } };
    const abortCombo = () => {
      clearHold();
      const r = run.current;
      if (!r?.held) return;
      if (r.draft.said) handlers.current.stop();
      else handlers.current.cancel();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Alt") {
        if (timer !== null || run.current?.held) abortCombo();
        return;
      }
      if (!isBareOptionDown(event)) { abortCombo(); return; }
      const { phase } = handlers.current.turn;
      if ((phase !== "idle" && phase !== "error") || timer !== null || run.current) return;
      if (!canHoldTarget(field.current, host.current, btn.current)) return;
      timer = setTimeout(() => {
        timer = null;
        const cur = handlers.current.turn.phase;
        if ((cur === "idle" || cur === "error") && !run.current && canHoldTarget(field.current, host.current, btn.current)) {
          void handlers.current.start(true);
        }
      }, HOLD_OPTION_MS);
    };
    const onKeyUp = (event: KeyboardEvent) => {
      if (event.key !== "Alt" && event.altKey) return;
      clearHold();
      if (run.current?.held) handlers.current.stop();
    };
    const onPointerDown = () => { if (timer !== null || run.current?.held) abortCombo(); };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", abortCombo);
    return () => {
      clearHold();
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("blur", abortCombo);
    };
  }, []);

  // Escape stops it without waiting for the last words.
  useEffect(() => {
    if (!ACTIVE.has(turn.phase)) return;
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); cancel(); } };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [turn.phase]);

  // Typing, deleting, or moving the caret re-anchors the next transcript chunk.
  useEffect(() => {
    const node = field.current;
    if (!node || !ACTIVE.has(turn.phase)) return;
    const sync = () => {
      const r = run.current;
      if (!r || r.writing) return;
      r.draft = reanchorDraft(r.draft, node.value, node.selectionStart, node.selectionEnd);
    };
    node.addEventListener("input", sync);
    node.addEventListener("keyup", sync);
    node.addEventListener("mouseup", sync);
    return () => {
      node.removeEventListener("input", sync);
      node.removeEventListener("keyup", sync);
      node.removeEventListener("mouseup", sync);
    };
  }, [turn.phase]);

  // The timer beside the field, once a second.
  useEffect(() => {
    if (turn.phase !== "listening") return;
    const tick = () => setNow(performance.now());
    tick();
    const timer = setInterval(tick, 1000);
    return () => clearInterval(timer);
  }, [turn.phase]);

  // A finished dictation and its error clear by themselves; typing clears the error too.
  useEffect(() => {
    if (turn.phase !== "settling" && turn.phase !== "error") return;
    const timer = setTimeout(() => setTurn(IDLE), turn.phase === "settling" ? 320 : 4000);
    const node = field.current, typed = () => setTurn(IDLE);
    if (turn.phase === "error") node?.addEventListener("keydown", typed);
    return () => { clearTimeout(timer); node?.removeEventListener("keydown", typed); };
  }, [turn.phase]);

  // The animation plays while the turn has something to show.
  useEffect(() => {
    const node = canvas.current, box = host.current;
    if (!node || !box || turn.phase === "idle" || !motionAllowed()) return;
    return glow.current.play(node, box, () => ({ phase: turn.phase, mic: run.current?.mic ?? null }));
  }, [turn.phase]);

  useEffect(() => () => {
    const r = run.current;
    if (!r) return;
    finish(r);
    if (r.id !== null) void invoke("dictation_cancel", { id: r.id }).catch(() => {});
  }, []);

  const press = () => {
    if (turn.phase === "listening") stop();
    else if (turn.phase === "starting") cancel();
    else if (turn.phase !== "thinking") void start();
  };
  const status = turn.phase === "listening" ? elapsed(now - (run.current?.startedAt ?? now)) : turn.phase === "thinking" ? "thinking" : turn.phase === "error" ? turn.error : null;
  const look = turn.phase === "starting" || turn.phase === "settling" ? "idle" : turn.phase;
  const label = turn.phase === "listening" ? "Stop dictation (Esc cancels)" : turn.phase === "starting" ? "Cancel dictation" : "Dictate prompt";
  const optKey = APPLE ? "⌥" : "Alt";
  const title = turn.phase === "listening" ? (run.current?.held ? `Release ${optKey} to finish (Esc cancels)` : label) : turn.phase === "starting" ? label : `Dictate prompt (or hold ${optKey})`;
  const box = host.current;

  return <>
    <button ref={btn} type="button" className="dict-btn" data-state={look} title={title} aria-label={label} aria-pressed={turn.phase === "listening"} disabled={turn.phase === "thinking"} onMouseDown={event => event.preventDefault()} onClick={press}>
      {look === "listening" ? <Icon name="stop" /> : look === "thinking" ? <span className="dict-dots" aria-hidden="true"><i /><i /><i /></span> : look === "error" ? <Icon name="micOff" /> : <Icon name="mic" />}
    </button>
    {box && createPortal(<>
      <canvas ref={canvas} className="dict-canvas" aria-hidden="true" hidden={turn.phase === "idle"} />
      <span className={`dict-status${status === null ? "" : ` shown ${turn.phase}`}`} role="status"><span className="dot" />{status}</span>
    </>, box)}
  </>;
}

/// Whether this composer should answer a held Option/Alt key on screen now.
function canHoldTarget(field: HTMLTextAreaElement | null, box: HTMLElement | null, button: HTMLButtonElement | null): boolean {
  if (!field || !box || !button || !field.isConnected || field.disabled || field.readOnly || document.hidden) return false;
  const rect = box.getBoundingClientRect();
  if (rect.width <= 0 || rect.height <= 0 || getComputedStyle(box).visibility === "hidden") return false;
  const openDialog = document.querySelector("dialog[open]");
  if (openDialog && !openDialog.contains(box)) return false;

  const active = document.activeElement;
  if (active === field) return true;
  if (active instanceof Element && (active.closest(".tm-term") || active.closest("input, textarea, select, [contenteditable='true']"))) return false;

  const visible = [...document.querySelectorAll<HTMLButtonElement>(".dict-btn")].filter(candidate => {
    if (candidate.disabled || (openDialog && !openDialog.contains(candidate))) return false;
    const r = candidate.getBoundingClientRect();
    return r.width > 0 && r.height > 0 && getComputedStyle(candidate).visibility !== "hidden";
  });
  return visible.at(-1) === button;
}

async function openMic(take: (pcm: Int16Array) => void, lost: () => void): Promise<Mic> {
  const stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true } });
  const context = new AudioContext();
  const source = context.createMediaStreamSource(stream);
  const analyser = context.createAnalyser();
  analyser.fftSize = 256;
  // ScriptProcessor runs in every webview Beep ships in, without a worklet module to load.
  const processor = context.createScriptProcessor(4096, 1, 1);
  const resampler = new Resampler(context.sampleRate);
  const mute = context.createGain();
  mute.gain.value = 0;
  processor.onaudioprocess = event => {
    const pcm = resampler.push(event.inputBuffer.getChannelData(0));
    if (pcm.length) take(pcm);
  };
  source.connect(analyser);
  source.connect(processor);
  processor.connect(mute);
  mute.connect(context.destination);
  for (const track of stream.getAudioTracks()) track.addEventListener("ended", lost);

  return { stream, context, analyser, levels: new Uint8Array(analyser.frequencyBinCount) };
}

function closeMic(mic: Mic) {
  for (const track of mic.stream.getTracks()) track.stop();
  void mic.context.close().catch(() => {});
}

// ----- The animation -----

/// Characters from dense to light: the mound's body, its slopes and its spray.
const DENSE = "#$%&8BMW*@", MID = "opqkhdb0XZUYcvunxrjftLCQ2469", LIGHT = "/\\|()?+~-;:,.'`";

interface Particle { sx: number; sy: number; cx: number; cy: number; tx: number; ty: number; born: number; dur: number; size: number }
interface Stipple { x: number; y: number; born: number; dur: number; size: number }
interface Frame { phase: Turn["phase"]; mic: Mic | null }

interface Floor { w: number; h: number; centerX: number; cols: number; baseY: number }

/// The canvas over a composer: the mound at the foot, the words flying in.
class Glow {
  private energy = 0;
  private tint = 0;
  private particles: Particle[] = [];
  private stipples: Stipple[] = [];
  private measure: CanvasRenderingContext2D | null = null;
  private floor: Floor = { w: 0, h: 0, centerX: 0, cols: 36, baseY: 0 };

  /// Draws until the turn ends and the last word lands. Returns the stop.
  play(canvas: HTMLCanvasElement, box: HTMLElement, frame: () => Frame): () => void {
    const ctx = canvas.getContext("2d");
    if (!ctx) return () => {};
    let raf = 0;
    const colors = themeColors(box);

    const draw = (now: number) => {
      raf = 0;
      const { phase, mic } = frame();
      const target = phase === "listening" ? micEnergy(mic) : phase === "thinking" ? 0.16 : 0;
      this.energy += (target - this.energy) * (target > this.energy ? 0.24 : phase === "settling" ? 0.2 : 0.09);
      this.tint = phase === "error" ? 1 : this.tint * 0.9;
      const rect = box.getBoundingClientRect(), dpr = window.devicePixelRatio || 1, w = Math.round(rect.width), h = Math.round(rect.height);
      if (canvas.width !== w * dpr || canvas.height !== h * dpr) { canvas.width = w * dpr; canvas.height = h * dpr; }
      if (this.floor.w !== w || this.floor.h !== h) this.floor = measureFloor(box, rect, w, h);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);
      const [r, g, b] = colors.fg.map((c, i) => Math.round(c * (1 - this.tint) + colors.err[i] * this.tint));
      const rgba = (a: number) => `rgba(${r}, ${g}, ${b}, ${a.toFixed(3)})`;

      const level = phase === "listening" ? Math.max(0.15, this.energy) : this.energy;
      if (level > 0.015) this.mound(ctx, now, level, colors.mono, rgba);
      this.particles = this.particles.filter(p => this.particle(ctx, p, now, rgba));
      this.stipples = this.stipples.filter(s => this.stipple(ctx, s, now, rgba));

      if (phase === "listening" || phase === "thinking" || phase === "starting" || this.energy > 0.015 || this.particles.length || this.stipples.length) raf = requestAnimationFrame(draw);
      else ctx.clearRect(0, 0, w, h);
    };
    raf = requestAnimationFrame(draw);

    return () => { if (raf) cancelAnimationFrame(raf); };
  }

  /// Sends `word`, written after `before` in `field`, from the mound to its place.
  spawn(box: HTMLElement, field: HTMLTextAreaElement, before: string, word: string) {
    if (!motionAllowed()) return;
    const rect = box.getBoundingClientRect(), w = Math.round(rect.width), h = Math.round(rect.height);
    if (this.floor.w !== w || this.floor.h !== h) this.floor = measureFloor(box, rect, w, h);
    const target = this.wordRect(box, field, before, word), now = performance.now();
    const moundX = this.floor.centerX, moundY = this.floor.baseY - 3 - (1.8 + Math.max(this.energy, 0.45) * 5.6) * 8.2 * 0.72;
    const count = Math.min(26, Math.max(12, word.length * 3));

    for (let i = 0; i < count; i++) {
      const u = (i + Math.random() * 0.65) / count;
      const sx = moundX + (Math.random() - 0.5) * 54, sy = moundY + (Math.random() - 0.5) * 12;
      const tx = target.x + u * target.w + (Math.random() - 0.5) * 4, ty = target.y + (Math.random() - 0.5) * target.h * 0.75;
      // An arc above the straight line, like a fountain.
      this.particles.push({ sx, sy, tx, ty, cx: sx * 0.55 + tx * 0.45 + (Math.random() - 0.5) * 24, cy: Math.min(sy, ty) - 18 - Math.random() * 26, born: now + u * 85 + Math.random() * 35, dur: 270 + Math.random() * 140, size: Math.random() < 0.35 ? 2.4 : 1.8 });
    }

    const dots = Math.min(34, Math.max(14, word.length * 4));
    for (let i = 0; i < dots; i++) this.stipples.push({ x: target.x + Math.random() * target.w, y: target.y + (Math.random() - 0.5) * target.h * 0.85, born: now + 150 + Math.random() * 130, dur: 210 + Math.random() * 130, size: Math.random() < 0.4 ? 2.1 : 1.5 });
  }

  private mound(ctx: CanvasRenderingContext2D, now: number, level: number, mono: string, rgba: (a: number) => string) {
    const cellW = 5.6, cellH = 8, maxRows = 8;
    const { cols, centerX, baseY } = this.floor;
    const startX = centerX - cols * cellW * 0.5, tick = Math.floor(now / (level > 0.35 ? 48 : 85)), half = (cols - 1) * 0.5;
    ctx.font = `500 9.5px ${mono}`;
    ctx.textBaseline = "alphabetic";

    for (let c = 0; c < cols; c++) {
      const nx = (c - half) / half;
      const bell = Math.exp(-nx * nx * 2.9) * (1 - nx * nx * 0.32);
      const wave = Math.sin(nx * 5.2 + now * 0.007) * 0.12 * level + Math.cos(nx * 9.5 - now * 0.011) * 0.08 * level;
      const height = (1.55 + level * 6.1) * Math.max(0, bell + wave);

      for (let row = 0; row < Math.min(maxRows, Math.ceil(height)); row++) {
        const density = (1 - row / Math.max(1, height)) * bell;
        if (density < 0.05) continue;
        const [chars, alpha] = density > 0.52 ? [DENSE, Math.min(0.92, 0.52 + density * 0.45)] : density > 0.24 ? [MID, 0.26 + density * 0.52] : [LIGHT, 0.11 + density * 0.36];
        ctx.fillStyle = rgba(alpha * Math.min(1, level * 3.2));
        ctx.fillText(chars[Math.floor(hash(c, row, tick) * chars.length) % chars.length], startX + c * cellW, baseY - row * cellH);
      }
    }
  }

  /// Draws a particle's head and trail; false once it has landed.
  private particle(ctx: CanvasRenderingContext2D, p: Particle, now: number, rgba: (a: number) => string) {
    const t = (now - p.born) / p.dur;
    if (t < 0) return true;
    if (t >= 1) return false;

    for (let s = 2; s >= 0; s--) {
      const ts = Math.max(0, t - s * 0.045), inv = 1 - ts;
      const x = inv * inv * p.sx + 2 * inv * ts * p.cx + ts * ts * p.tx, y = inv * inv * p.sy + 2 * inv * ts * p.cy + ts * ts * p.ty;
      const size = s === 0 ? p.size : p.size * 0.78;
      ctx.fillStyle = rgba(Math.sin(Math.min(1, ts) * Math.PI) * (s === 0 ? 0.95 : s === 1 ? 0.38 : 0.16));
      ctx.fillRect(x - size * 0.5, y - size * 0.5, size, size);
    }

    return true;
  }

  private stipple(ctx: CanvasRenderingContext2D, s: Stipple, now: number, rgba: (a: number) => string) {
    const t = (now - s.born) / s.dur;
    if (t < 0) return true;
    if (t >= 1) return false;

    ctx.fillStyle = rgba(Math.sin(t * Math.PI) * 0.85);
    ctx.fillRect(s.x, s.y, s.size, s.size);
    return true;
  }

  /// Where `word` sits in the field, in the box's coordinates, by laying out the text as the field wraps it.
  private wordRect(box: HTMLElement, field: HTMLTextAreaElement, before: string, word: string) {
    this.measure ??= document.createElement("canvas").getContext("2d");
    const m = this.measure, cs = getComputedStyle(field);
    const padL = parseFloat(cs.paddingLeft) || 0, padR = parseFloat(cs.paddingRight) || 0, padT = parseFloat(cs.paddingTop) || 0;
    const lineHeight = parseFloat(cs.lineHeight) || 20, maxW = Math.max(120, field.clientWidth - padL - padR);
    if (!m) return { x: 0, y: 0, w: 0, h: lineHeight };
    m.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`;
    let line = "", row = 0, x = 0, seen = 0;

    for (const token of (before + word).split(/(\s+)/)) {
      if (!token) continue;
      if (token === "\n" || /\n/.test(token)) { row += token.split("\n").length - 1; line = ""; seen += token.length; continue; }
      const next = line + token;
      if (line && !/^\s+$/.test(token) && m.measureText(next).width > maxW) { row++; line = token; }
      else line = next;
      if (seen === before.length) x = m.measureText(line.slice(0, line.length - token.length)).width;
      seen += token.length;
    }

    const w = Math.min(Math.max(14, m.measureText(word).width), maxW), fr = field.getBoundingClientRect(), br = box.getBoundingClientRect();
    return { x: fr.left - br.left + padL + Math.min(x, maxW - w), y: fr.top - br.top + padT + (row * lineHeight - field.scrollTop) + lineHeight * 0.52, w, h: lineHeight * 0.72 };
  }
}

/// Finds the open span along the bottom controls row so the mound stays clear of pills and buttons.
function measureFloor(box: HTMLElement, rect: DOMRect, w: number, h: number): Floor {
  const cellW = 5.6;
  let left = 12, right = w - 12, controlsTop = h;

  for (const node of box.querySelectorAll<HTMLElement>(":scope > :is(button, #agmode, #agmodel, #agstop), :scope > .row > :not(.sp)")) {
    const cr = node.getBoundingClientRect();
    if (cr.width <= 0 || cr.height <= 0 || cr.bottom <= rect.bottom - 42) continue;
    controlsTop = Math.min(controlsTop, cr.top - rect.top);
    if ((cr.left + cr.right) * 0.5 - rect.left < w * 0.5) left = Math.max(left, cr.right - rect.left + 10);
    else right = Math.min(right, cr.left - rect.left - 10);
  }

  const gap = right - left;
  if (gap >= 140) {
    const halfW = Math.min(27 * cellW, gap * 0.5);
    const centerX = Math.max(left + halfW, Math.min(right - halfW, w * 0.5));
    return { w, h, centerX, cols: Math.max(20, Math.floor((halfW * 2) / cellW)), baseY: h - 5 };
  }

  return { w, h, centerX: w * 0.5, cols: Math.min(54, Math.max(20, Math.floor((w - 32) / cellW))), baseY: Math.max(24, controlsTop - 4) };
}

/// The microphone's loudness, 0 to 1, from its low frequencies.
function micEnergy(mic: Mic | null) {
  if (!mic) return 0.15;
  mic.analyser.getByteFrequencyData(mic.levels);
  const bins = Math.min(48, mic.levels.length);
  let sum = 0;
  for (let i = 1; i < bins; i++) sum += mic.levels[i] ?? 0;
  return Math.min(1, Math.max(0.14, (sum / (bins * 255)) * 2.9));
}

/// A stable pseudo-random value per cell and tick, for the characters' shimmer.
function hash(x: number, y: number, tick: number) {
  let h = (x * 374761393 + y * 668265263 + tick * 1442695041) | 0;
  h = (h ^ (h >>> 13)) * 1274126177;
  return ((h ^ (h >>> 16)) >>> 0) / 4294967295;
}

/// The skin's text and error colors as RGB, and its mono font, read once per play.
function themeColors(box: HTMLElement) {
  const cs = getComputedStyle(box);
  return { fg: rgb(cs.color, [227, 228, 230]), err: rgb(cs.getPropertyValue("--err"), [235, 87, 87]), mono: cs.getPropertyValue("--mono").trim() || "ui-monospace, monospace" };
}

function rgb(css: string, fallback: number[]) {
  const probe = document.createElement("span");
  probe.style.color = css.trim();
  probe.style.display = "none";
  document.body.append(probe);
  const match = getComputedStyle(probe).color.match(/(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/);
  probe.remove();
  return match ? [Number(match[1]), Number(match[2]), Number(match[3])] : fallback;
}

