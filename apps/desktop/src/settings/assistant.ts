// The Home assistant's harness, model and thinking level, the same on every machine.
import { invoke } from "../ui/tauri.ts";

/// A null field is left to the app: the first harness, its own model and thinking level.
export interface AssistantPick { agent: string | null; model: string | null; effort: string | null }

export function loadAssistant(): Promise<AssistantPick> {
  return invoke<AssistantPick>("assistant_settings_get");
}

export async function saveAssistant(pick: AssistantPick): Promise<void> {
  await invoke("assistant_settings_save", { settings: pick });
}
