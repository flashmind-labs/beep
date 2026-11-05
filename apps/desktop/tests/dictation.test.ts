import assert from "node:assert/strict";
import { test } from "node:test";
import { DICTATION_NOTE, MODEL_ROWS, NO_KEYS, active, filterModels, shownModel, type DictationView } from "../src/assistant/dictation-state.ts";

const offered = [
  { provider: "openrouter" as const, name: "OpenRouter", default_model: "openai/whisper-1" },
  { provider: "openai" as const, name: "OpenAI", default_model: "gpt-4o-mini-transcribe" },
];
const view = (provider: DictationView["provider"], model = ""): DictationView => ({ provider, model, offered });

test("with no provider picked dictation is off and shows no model", () => {
  assert.equal(active(view(null)), null);
  assert.equal(shownModel(view(null, "x")), "");
});

test("a provider whose key was removed counts as off", () => {
  assert.equal(active(view("mistral")), null);
});

test("an empty model shows the provider's default, a typed one wins", () => {
  assert.equal(shownModel(view("openai")), "gpt-4o-mini-transcribe");
  assert.equal(shownModel(view("openai", "  gpt-4o-transcribe ")), "gpt-4o-transcribe");
});

test("the model search matches every word in the name or id, and caps the rows", () => {
  const models = [
    { id: "openai/whisper-1", name: "Whisper 1" },
    { id: "mistralai/voxtral-mini-transcribe", name: "Voxtral Mini Transcribe" },
    ...Array.from({ length: MODEL_ROWS + 5 }, (_, i) => ({ id: `acme/stt-${i}`, name: `Acme ${i}` })),
  ];
  assert.deepEqual(filterModels(models, "mistral transcribe").rows.map(m => m.id), ["mistralai/voxtral-mini-transcribe"]);
  assert.deepEqual(filterModels(models, "WHISPER").rows.map(m => m.id), ["openai/whisper-1"]);
  const all = filterModels(models, "");
  assert.equal(all.rows.length, MODEL_ROWS);
  assert.equal(all.more, models.length - MODEL_ROWS);
  assert.deepEqual(filterModels(models, "nothing like it"), { rows: [], more: 0 });
});

test("the section's words are plain", () => {
  assert.doesNotMatch(`${NO_KEYS} ${DICTATION_NOTE}`, /[—→⇒←]/);
});
