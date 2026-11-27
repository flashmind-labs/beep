// The Home assistant's harness, model and thinking level, the same on every machine.
import { invoke } from "../../ui/tauri.ts";

/// A null field is left to the app: the first harness, its own model and thinking level.
export interface AssistantPick { agent: string | null; model: string | null; effort: string | null; fast: boolean | null }

export function loadAssistant(): Promise<AssistantPick> {
  return invoke<AssistantPick>("assistant_settings_get");
}

/// Told of each save, so what shows the setting reads it again.
const saves = new EventTarget();

export async function saveAssistant(pick: AssistantPick): Promise<void> {
  await invoke("assistant_settings_save", { settings: pick });
  saves.dispatchEvent(new Event("saved"));
}

/// Calls `listener` after each save of the setting; returns what stops it.
export function onAssistantSaved(listener: () => void): () => void {
  saves.addEventListener("saved", listener);
  return () => saves.removeEventListener("saved", listener);
}
