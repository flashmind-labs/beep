// Voice dictation in the prompt composers: the shared setting, the models it
// can use, how a transcript lands in the field, and microphone audio as the
// 16 kHz mono 16-bit PCM the app streams to the speech-to-text provider.
import { createStore } from "zustand/vanilla";

/// `settings/dictation` (DictationSettings): `model` null picks the first provider with a key.
export interface DictationSettings { enabled: boolean; model: string | null }

export type DictationProvider = "elevenlabs" | "mistral" | "openrouter";

export interface DictationModel { id: string; provider: DictationProvider; label: string }

export const DICTATION_MODELS: readonly DictationModel[] = [
  { id: "elevenlabs:scribe_v3_live", provider: "elevenlabs", label: "ElevenLabs · Scribe v3 Live" },
  { id: "elevenlabs:scribe_v2_realtime", provider: "elevenlabs", label: "ElevenLabs · Scribe v2 Realtime" },
  { id: "mistral:voxtral-mini-transcribe-realtime-3-0", provider: "mistral", label: "Mistral · Voxtral Mini Realtime" },
  { id: "openrouter:openai/whisper-large-v3-turbo", provider: "openrouter", label: "OpenRouter · Whisper Large v3 Turbo" },
  { id: "openrouter:deepgram/nova-3", provider: "openrouter", label: "OpenRouter · Deepgram Nova-3" },
];

/// The order the app tries keys in when no model is picked.
const AUTO_ORDER: readonly DictationProvider[] = ["elevenlabs", "mistral", "openrouter"];

export const PROVIDER_NAMES: Record<DictationProvider, string> = { elevenlabs: "ElevenLabs", mistral: "Mistral", openrouter: "OpenRouter" };

/// The provider a model id names; a bare id is ElevenLabs's, as the app reads it.
export function providerOf(model: string): DictationProvider | null {
  const [prefix, rest] = model.includes(":") ? model.split(":", 2) : ["elevenlabs", model];
  if (!rest) return null;
  return prefix === "elevenlabs" || prefix === "mistral" || prefix === "openrouter" ? prefix : null;
}

/// The saved API keys' last four characters, by provider.
export type DictationKeys = Partial<Record<DictationProvider, string>>;

/// The provider a dictation would use now, or null when no key fits.
export function activeProvider(model: string | null, keys: DictationKeys): DictationProvider | null {
  if (model !== null) {
    const provider = providerOf(model);
    return provider && keys[provider] !== undefined ? provider : null;
  }

  return AUTO_ORDER.find(provider => keys[provider] !== undefined) ?? null;
}

/// The line under the setting.
export function dictationNote(settings: DictationSettings, keys: DictationKeys): string {
  if (!settings.enabled) return "Dictate prompts with ElevenLabs, Mistral or OpenRouter speech-to-text";
  const wanted = settings.model === null ? null : providerOf(settings.model);
  const provider = activeProvider(settings.model, keys);
  if (provider === null) return wanted ? `Add your ${PROVIDER_NAMES[wanted]} API key in Credentials to start dictating` : "Add an ElevenLabs, Mistral or OpenRouter API key in Credentials to start dictating";
  const label = DICTATION_MODELS.find(model => model.id === settings.model)?.label ?? `${PROVIDER_NAMES[provider]} · default model`;
  return `${label} · mic button in prompt composers`;
}

export function parseSettings(value: unknown): DictationSettings {
  if (typeof value !== "object" || value === null || !("enabled" in value) || typeof value.enabled !== "boolean") throw new Error("The app sent unreadable dictation settings.");
  const model = "model" in value && typeof value.model === "string" ? value.model : null;
  return { enabled: value.enabled, model };
}

/// The dictation keys among `list_credentials`' rows.
export function keysFrom(list: unknown): DictationKeys {
  const keys: DictationKeys = {};
  if (!Array.isArray(list)) return keys;
  const rows: unknown[] = list;

  for (const row of rows) {
    if (typeof row !== "object" || row === null || !("kind" in row) || row.kind !== "api_key" || !("provider" in row) || typeof row.provider !== "string") continue;
    if ("unreadable" in row && row.unreadable === true) continue;
    const provider: string = row.provider;
    if (provider === "elevenlabs" || provider === "mistral" || provider === "openrouter") keys[provider] = "last4" in row && typeof row.last4 === "string" ? row.last4 : "";
  }

  return keys;
}

type Call = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

interface SettingsState { saved: DictationSettings | null; keys: DictationKeys; busy: boolean; error: string }

/// The shared setting: loaded once per window, saved through the mesh.
export function createDictationSettings(call: Call) {
  const state = createStore<SettingsState>()(() => ({ saved: null, keys: {}, busy: false, error: "" }));
  let loading: Promise<void> | null = null;

  const save = async (next: DictationSettings) => {
    const was = state.getState().saved;
    if (state.getState().busy || was === null) return;
    state.setState({ saved: next, busy: true, error: "" });
    try { await call("dictation_settings_save", { settings: next }); }
    catch (error) { state.setState({ saved: was, error: String(error) }); }
    finally { state.setState({ busy: false }); }
  };

  return {
    state,
    load(): Promise<void> {
      loading ??= (async () => {
        state.setState({ error: "" });
        try {
          const [settings, list] = await Promise.all([call<unknown>("dictation_settings_get"), call<unknown>("list_credentials").catch(() => [])]);
          state.setState({ saved: parseSettings(settings), keys: keysFrom(list) });
        } catch (error) { state.setState({ error: String(error) }); }
        finally { loading = null; }
      })();
      return loading;
    },
    setEnabled(enabled: boolean) { const saved = state.getState().saved; if (saved) void save({ ...saved, enabled }); },
    setModel(model: string | null) { const saved = state.getState().saved; if (saved) void save({ ...saved, model }); },
  };
}

// ----- A dictation turn -----

/// What the app streams back for one dictation (DictationEvent).
export type DictationEvent =
  | { kind: "ready" }
  | { kind: "transcript"; committed: string; partial: string }
  | { kind: "thinking" }
  | { kind: "done"; nothing_heard: boolean }
  | { kind: "error"; message: string };

export function parseEvent(value: unknown): DictationEvent | null {
  if (typeof value !== "object" || value === null || !("kind" in value)) return null;

  switch (value.kind) {
    case "ready": return { kind: "ready" };
    case "thinking": return { kind: "thinking" };
    case "transcript": return "committed" in value && "partial" in value && typeof value.committed === "string" && typeof value.partial === "string" ? { kind: "transcript", committed: value.committed, partial: value.partial } : null;
    case "done": return { kind: "done", nothing_heard: "nothing_heard" in value && value.nothing_heard === true };
    case "error": return { kind: "error", message: "message" in value && typeof value.message === "string" ? value.message : "Dictation failed." };
    default: return null;
  }
}

/// `starting` waits for the microphone and the provider; `settling` lets the animation finish.
export type DictationPhase = "idle" | "starting" | "listening" | "thinking" | "settling" | "error";

export interface Turn { phase: DictationPhase; error: string }

export const IDLE: Turn = { phase: "idle", error: "" };

export function nextTurn(turn: Turn, event: DictationEvent): Turn {
  switch (event.kind) {
    case "ready": return turn.phase === "starting" ? { phase: "listening", error: "" } : turn;
    case "transcript": return turn;
    case "thinking": return turn.phase === "listening" || turn.phase === "starting" ? { phase: "thinking", error: "" } : turn;
    case "done": return event.nothing_heard ? { phase: "error", error: "Nothing heard." } : { phase: "settling", error: "" };
    case "error": return { phase: "error", error: event.message };
  }
}

/// The field's text around the caret when dictation started.
export interface Anchor { before: string; after: string }

export function anchorOf(value: string, start: number, end: number): Anchor {
  return { before: value.slice(0, start), after: value.slice(end) };
}

/// The field with the transcript so far in place of the selection, spaced from
/// the text around it. `caret` is where the transcript ends.
export function spliceTranscript(anchor: Anchor, committed: string, partial: string): { value: string; caret: number } {
  const said = joinSaid(committed, partial);
  if (!said) return { value: anchor.before + anchor.after, caret: anchor.before.length };
  const lead = anchor.before && !/\s$/.test(anchor.before) ? " " : "";
  const tail = anchor.after && !/^\s/.test(anchor.after) ? " " : "";
  const head = anchor.before + lead + said;
  return { value: head + tail + anchor.after, caret: head.length };
}

function joinSaid(committed: string, partial: string) {
  const a = committed.trim(), b = partial.trim();
  return a && b ? `${a} ${b}` : a || b;
}

/// The words `next` adds at the end of `prev`, with the text before each: the
/// ones the animation flies in. A rewritten partial adds nothing.
export function addedWords(prev: string, next: string): { before: string; word: string }[] {
  if (!next.startsWith(prev)) return [];
  const added: { before: string; word: string }[] = [];
  const pattern = /\S+/g;
  for (let match = pattern.exec(next); match; match = pattern.exec(next)) {
    if (match.index + match[0].length <= prev.length) continue;
    if (match.index < prev.length) continue;
    added.push({ before: next.slice(0, match.index), word: match[0] });
  }
  return added;
}

// ----- Microphone audio -----

export const SAMPLE_RATE = 16_000;
/// The app takes chunks of at most 256 KB.
export const MAX_CHUNK_BYTES = 262_144;

/// Turns float samples at the microphone's rate into 16 kHz 16-bit PCM,
/// averaging each output sample's span. Keeps its place across buffers.
export class Resampler {
  private readonly step: number;
  private pos = 0;
  private sum = 0;
  private count = 0;

  constructor(inputRate: number) {
    this.step = inputRate / SAMPLE_RATE;
  }

  push(input: Float32Array): Int16Array {
    const out: number[] = [];

    for (const sample of input) {
      this.sum += sample;
      this.count++;
      this.pos++;
      if (this.pos < this.step) continue;

      this.pos -= this.step;
      const mean = this.sum / this.count;
      const clamped = Math.max(-1, Math.min(1, mean));
      out.push(clamped < 0 ? Math.round(clamped * 0x8000) : Math.round(clamped * 0x7fff));
      this.sum = 0;
      this.count = 0;
    }

    return Int16Array.from(out);
  }
}

/// Little-endian bytes of `pcm`, base64-encoded, split to the app's chunk limit.
export function pcmChunks(pcm: Int16Array): string[] {
  const bytes = new Uint8Array(pcm.length * 2);
  const view = new DataView(bytes.buffer);
  pcm.forEach((sample, i) => view.setInt16(i * 2, sample, true));
  const chunks: string[] = [];

  for (let at = 0; at < bytes.length; at += MAX_CHUNK_BYTES) chunks.push(base64(bytes.subarray(at, at + MAX_CHUNK_BYTES)));

  return chunks;
}

function base64(bytes: Uint8Array) {
  let text = "";
  for (let at = 0; at < bytes.length; at += 0x8000) text += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  return btoa(text);
}

/// `m:ss` for the elapsed time beside the field.
export function elapsed(ms: number): string {
  const secs = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
}
