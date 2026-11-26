import test from "node:test";
import assert from "node:assert/strict";
import { activeModel, activeProvider, addedWords, anchorOf, applyTranscript, createDictationSettings, dictationNote, IDLE, isBareOptionDown, keysFrom, nextTurn, parseEvent, parseSettings, pausePlayingMedia, pcmChunks, providerOf, reanchorDraft, Resampler, resumePausedMedia, spliceTranscript, startDraft, MAX_CHUNK_BYTES, type DownloadChannel, type PlayableMedia } from "../src/chat/dictation-state.ts";

test("only live local and cloud models resolve to a provider; legacy OpenRouter settings reset to Automatic", () => {
  assert.equal(providerOf("local:parakeet-unified"), "local");
  assert.equal(providerOf("local:nemotron-3.5-asr"), "local");
  assert.equal(providerOf("local:unknown"), null);
  assert.equal(providerOf("openrouter:openai/whisper-large-v3-turbo"), null);
  assert.equal(providerOf("mistral:voxtral-mini-transcribe-realtime-3-0"), "mistral");
  assert.equal(providerOf("scribe_v3_live"), "elevenlabs");
  assert.equal(providerOf("acme:model"), null);
  assert.deepEqual(parseSettings({ enabled: true, model: "openrouter:openai/whisper-large-v3-turbo" }), { enabled: true, model: null });
});

test("automatic prefers an installed local model, then a keyed live cloud provider", () => {
  const nemotronReady = { "local:nemotron-3.5-asr": { bytes: 751_094_240, installed: true, downloading: false, downloadedBytes: 751_094_240 } };
  assert.equal(activeModel(null, { elevenlabs: "4a9f" }, nemotronReady)?.id, "local:nemotron-3.5-asr");
  assert.equal(activeProvider(null, { mistral: "1111" }), "mistral");
  assert.equal(activeProvider("local:parakeet-unified", { elevenlabs: "4a9f" }, {}), null);
  assert.equal(activeProvider("local:nemotron-3.5-asr", {}, nemotronReady), "local");
  assert.equal(activeProvider("elevenlabs:scribe_v3_live", { mistral: "1111" }), null);
  assert.equal(activeProvider(null, {}), null);

  assert.match(dictationNote({ enabled: true, model: "local:parakeet-unified" }, {}, {}), /Download once \(731 MB\) to dictate live in English on this computer/);
  assert.match(dictationNote({ enabled: true, model: "local:nemotron-3.5-asr" }, {}, nemotronReady), /Ready on this computer/);
  assert.match(dictationNote({ enabled: true, model: "elevenlabs:scribe_v3_live" }, {}), /Add your ElevenLabs API key/);
  assert.match(dictationNote({ enabled: true, model: null }, { mistral: "9c12" }), /Live via Mistral/);
});

test("only readable live dictation keys count", () => {
  const keys = keysFrom([
    { kind: "api_key", provider: "elevenlabs", last4: "4a9f" },
    { kind: "api_key", provider: "openrouter", last4: "9c12" },
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

  draft = reanchorDraft(draft, "I am ", 5, 5);
  step = applyTranscript(draft, "I am ", 5, 5, "I am dictating. And now I'm not.", "");
  draft = step.draft;
  assert.equal(step.write?.value, "I am And now I'm not.");
  assert.deepEqual(step.write?.words.map(w => w.word), ["And", "now", "I'm", "not."]);

  step = applyTranscript(draft, "I am ready: ", 12, 12, "I am dictating. And now I'm not. ship it", "");
  assert.equal(step.write?.value, "I am ready: ship it");
});

test("deleting an uncommitted partial skips its later commit", () => {
  let draft = startDraft("", 0, 0);
  let step = applyTranscript(draft, "", 0, 0, "", "1 am Dikta");
  draft = step.draft;
  assert.equal(step.write?.value, "1 am Dikta");

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

test("a failed save puts the setting back and local model downloads track progress and cancellation", async () => {
  const calls: string[] = [];
  const channels: DownloadChannel[] = [];
  const store = createDictationSettings(
    async <T,>(cmd: string): Promise<T> => {
      calls.push(cmd);
      if (cmd === "dictation_settings_get") return { enabled: false, model: null } as T;
      if (cmd === "list_credentials") return [{ kind: "api_key", provider: "mistral", last4: "9c12" }] as T;
      if (cmd === "dictation_local_models") return [{ id: "local:parakeet-unified", bytes: 731_357_568, installed: false, downloading: false, downloaded_bytes: 0 }] as T;
      if (cmd === "dictation_model_download" || cmd === "dictation_model_cancel_download") return null as T;
      throw new Error("mesh is down");
    },
    () => {
      const channel: DownloadChannel = { onmessage() {} };
      channels.push(channel);
      return channel;
    },
  );
  await store.load();
  assert.deepEqual(store.state.getState().keys, { mistral: "9c12" });
  store.setEnabled(true);
  assert.equal(store.state.getState().saved?.enabled, true);
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(store.state.getState().saved?.enabled, false);
  assert.match(store.state.getState().error, /mesh is down/);

  await store.downloadModel("local:parakeet-unified");
  assert.equal(store.state.getState().localModels["local:parakeet-unified"]?.downloading, true);
  channels.at(-1)?.onmessage({ kind: "progress", downloaded_bytes: 300_000_000, total_bytes: 731_357_568 });
  assert.equal(store.state.getState().localModels["local:parakeet-unified"]?.downloadedBytes, 300_000_000);
  await store.cancelDownload("local:parakeet-unified");
  assert.equal(store.state.getState().localModels["local:parakeet-unified"]?.downloading, false);

  await store.downloadModel("local:parakeet-unified");
  channels.at(-1)?.onmessage({ kind: "done" });
  assert.equal(store.state.getState().localModels["local:parakeet-unified"]?.installed, true);
  assert.deepEqual(calls, [
    "dictation_settings_get",
    "list_credentials",
    "dictation_local_models",
    "dictation_settings_save",
    "dictation_model_download",
    "dictation_model_cancel_download",
    "dictation_model_download",
  ]);
});

test("only a bare non-repeating Option press starts a push-to-talk hold", () => {
  const base = { key: "Alt", repeat: false, metaKey: false, ctrlKey: false, shiftKey: false, isComposing: false };
  assert.equal(isBareOptionDown(base), true);
  assert.equal(isBareOptionDown({ ...base, repeat: true }), false);
  assert.equal(isBareOptionDown({ ...base, metaKey: true }), false);
  assert.equal(isBareOptionDown({ ...base, shiftKey: true }), false);
  assert.equal(isBareOptionDown({ ...base, key: "Tab" }), false);
});

test("pauses only playing media elements and skips ones resumed or ended before dictation finishes", () => {
  const make = (paused: boolean, ended = false) => {
    let curPaused = paused;
    let curEnded = ended;
    let plays = 0;
    const item: PlayableMedia & { plays(): number; setEnded(): void } = {
      get paused() { return curPaused; },
      get ended() { return curEnded; },
      pause() { curPaused = true; },
      play() { curPaused = false; plays++; return Promise.resolve(); },
      plays: () => plays,
      setEnded: () => { curEnded = true; },
    };
    return item;
  };

  const playing = make(false);
  const alreadyPaused = make(true);
  const finishedLater = make(false);

  const paused = pausePlayingMedia([playing, alreadyPaused, finishedLater]);
  assert.equal(paused.length, 2);
  assert.equal(playing.paused, true);
  assert.equal(finishedLater.paused, true);

  finishedLater.setEnded();
  resumePausedMedia(paused);
  assert.equal(playing.paused, false);
  assert.equal(playing.plays(), 1);
  assert.equal(alreadyPaused.paused, true);
  assert.equal(alreadyPaused.plays(), 0);
  assert.equal(finishedLater.plays(), 0);
});
