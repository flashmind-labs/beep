import test from "node:test";
import assert from "node:assert/strict";
import { activeProvider, addedWords, anchorOf, applyTranscript, createDictationSettings, dictationNote, IDLE, keysFrom, nextTurn, parseEvent, pcmChunks, providerOf, reanchorDraft, Resampler, spliceTranscript, startDraft, MAX_CHUNK_BYTES } from "../src/chat/dictation-state.ts";

test("a model names its provider; a bare id is ElevenLabs's", () => {
  assert.equal(providerOf("openrouter:openai/whisper-large-v3-turbo"), "openrouter");
  assert.equal(providerOf("mistral:voxtral-mini-transcribe-realtime-3-0"), "mistral");
  assert.equal(providerOf("scribe_v3_live"), "elevenlabs");
  assert.equal(providerOf("acme:model"), null);
});

test("automatic picks the first provider with a key; a picked model needs its own", () => {
  assert.equal(activeProvider(null, { openrouter: "9c12", mistral: "1111" }), "mistral");
  assert.equal(activeProvider("elevenlabs:scribe_v3_live", { openrouter: "9c12" }), null);
  assert.equal(activeProvider(null, {}), null);
  assert.match(dictationNote({ enabled: true, model: "elevenlabs:scribe_v3_live" }, {}), /Add your ElevenLabs API key/);
  assert.match(dictationNote({ enabled: true, model: null }, { openrouter: "9c12" }), /OpenRouter · default model/);
});

test("only readable dictation keys count", () => {
  const keys = keysFrom([
    { kind: "api_key", provider: "elevenlabs", last4: "4a9f" },
    { kind: "api_key", provider: "mistral", last4: "0000", unreadable: true },
    { kind: "api_key", provider: "anthropic", last4: "abcd" },
    { kind: "github", last4: "zzzz" },
    "junk",
  ]);
  assert.deepEqual(keys, { elevenlabs: "4a9f" });
});

test("the transcript replaces the selection, spaced from the text around it", () => {
  const anchor = anchorOf("Fix the bug please", 8, 11);
  assert.deepEqual(anchor, { before: "Fix the ", after: " please" });
  assert.deepEqual(spliceTranscript(anchor, "login", "crash"), { value: "Fix the login crash please", caret: 19 });
  assert.deepEqual(spliceTranscript(anchorOf("Hello", 5, 5), "world", ""), { value: "Hello world", caret: 11 });
  assert.deepEqual(spliceTranscript(anchorOf("Hello", 5, 5), "", ""), { value: "Hello", caret: 5 });
});

test("only words added at the end fly in", () => {
  assert.deepEqual(addedWords("open the", "open the file now"), [{ before: "open the ", word: "file" }, { before: "open the file ", word: "now" }]);
  assert.deepEqual(addedWords("open the fi", "open the file"), []);
  assert.deepEqual(addedWords("open a", "open the"), []);
});

test("deleting or editing words during dictation keeps the edit when speech continues", () => {
  let draft = startDraft("", 0, 0);
  let step = applyTranscript(draft, "", 0, 0, "I am dictating.", "");
  draft = step.draft;
  assert.equal(step.write?.value, "I am dictating.");

  // User deletes "dictating." while dictation is still running, leaving "I am ".
  draft = reanchorDraft(draft, "I am ", 5, 5);
  step = applyTranscript(draft, "I am ", 5, 5, "I am dictating. And now I'm not.", "");
  draft = step.draft;
  assert.equal(step.write?.value, "I am And now I'm not.");
  assert.deepEqual(step.write?.words.map(w => w.word), ["And", "now", "I'm", "not."]);

  // Deleting again and typing manually keeps both edits on the next phrase.
  step = applyTranscript(draft, "I am ready: ", 12, 12, "I am dictating. And now I'm not. ship it", "");
  assert.equal(step.write?.value, "I am ready: ship it");
});

test("deleting an uncommitted partial skips its later commit", () => {
  let draft = startDraft("", 0, 0);
  let step = applyTranscript(draft, "", 0, 0, "", "1 am Dikta");
  draft = step.draft;
  assert.equal(step.write?.value, "1 am Dikta");

  // User deletes "Dikta" before VAD commits the rewritten phrase "I am dictating.".
  draft = reanchorDraft(draft, "I am ", 5, 5);
  step = applyTranscript(draft, "I am ", 5, 5, "I am dictating.", "");
  draft = step.draft;
  assert.equal(step.write, null);

  step = applyTranscript(draft, "I am ", 5, 5, "I am dictating.", "done");
  assert.equal(step.write?.value, "I am done");
});

test("events move the turn and unknown ones are dropped", () => {
  const starting = { phase: "starting" as const, error: "" };
  const listening = nextTurn(starting, { kind: "ready" });
  assert.equal(listening.phase, "listening");
  assert.equal(nextTurn(listening, { kind: "thinking" }).phase, "thinking");
  assert.deepEqual(nextTurn(listening, { kind: "done", nothing_heard: true }), { phase: "error", error: "Nothing heard." });
  assert.equal(nextTurn(listening, { kind: "done", nothing_heard: false }).phase, "settling");
  assert.deepEqual(nextTurn(IDLE, { kind: "error", message: "Add your key" }), { phase: "error", error: "Add your key" });
  assert.equal(parseEvent({ kind: "transcript", committed: 1 }), null);
  assert.equal(parseEvent({ kind: "later" }), null);
  assert.deepEqual(parseEvent({ kind: "transcript", committed: "a", partial: "b" }), { kind: "transcript", committed: "a", partial: "b" });
});

test("48 kHz audio becomes 16 kHz 16-bit PCM across buffers", () => {
  const resampler = new Resampler(48_000);
  const first = resampler.push(new Float32Array(10).fill(1));
  const second = resampler.push(new Float32Array(5).fill(-1));
  assert.equal(first.length + second.length, 5);
  assert.equal(first[0], 0x7fff);
  assert.equal(second.at(-1), -0x8000);
});

test("PCM goes out little-endian in chunks the app accepts", () => {
  const [one] = pcmChunks(Int16Array.from([1, -2]));
  assert.deepEqual([...Buffer.from(one ?? "", "base64")], [1, 0, 0xfe, 0xff]);
  const many = pcmChunks(new Int16Array(MAX_CHUNK_BYTES));
  assert.equal(many.length, 2);
  for (const chunk of many) {
    const bytes = Buffer.from(chunk, "base64").length;
    assert.ok(bytes <= MAX_CHUNK_BYTES && bytes % 2 === 0);
  }
});

test("a failed save puts the setting back", async () => {
  const calls: string[] = [];
  const store = createDictationSettings(async <T,>(cmd: string): Promise<T> => {
    calls.push(cmd);
    if (cmd === "dictation_settings_get") return { enabled: false, model: null } as T;
    if (cmd === "list_credentials") return [{ kind: "api_key", provider: "openrouter", last4: "9c12" }] as T;
    throw new Error("mesh is down");
  });
  await store.load();
  assert.deepEqual(store.state.getState().keys, { openrouter: "9c12" });
  store.setEnabled(true);
  assert.equal(store.state.getState().saved?.enabled, true);
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(store.state.getState().saved?.enabled, false);
  assert.match(store.state.getState().error, /mesh is down/);
  assert.deepEqual(calls, ["dictation_settings_get", "list_credentials", "dictation_settings_save"]);
});
