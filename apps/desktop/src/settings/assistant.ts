// The Home assistant's harness, model and thinking level, the same on every machine.
import { invoke } from "../ui/tauri.ts";
import type { CredCheck } from "./credentials.ts";

/// A null field is left to the app: the first harness, its own model and thinking level.
export interface AssistantPick { agent: string | null; model: string | null; effort: string | null; fast: boolean | null }

/// The saved setting: the pick, and the Hermes connection that stands in for it while set.
export interface AssistantSetting extends AssistantPick { hermes: HermesConnection | null }

/// Where the assistant reaches Hermes: the machine that asks it, and the end of the saved token.
export interface HermesConnection { machine: string; machine_name: string; host: string; port: number; last4: string }

export function loadAssistant(): Promise<AssistantSetting> {
  return invoke<AssistantSetting>("assistant_settings_get");
}

/// Told of each save, so what shows the setting reads it again.
const saves = new EventTarget();

export async function saveAssistant(pick: AssistantPick): Promise<void> {
  await invoke("assistant_settings_save", { settings: pick });
  saves.dispatchEvent(new Event("saved"));
}

/// Where `connectHermes` reaches Hermes from: `machine` asks the dashboard at `host` and `port`.
export interface HermesAddress { machine: string; host: string; port: number; token: string }

/// Checks the Hermes dashboard from its machine and uses it for the assistant when it takes the
/// token; the check says why not.
export async function connectHermes(address: HermesAddress): Promise<CredCheck> {
  const check = await invoke<CredCheck>("hermes_connect", { ...address });
  if (check.state === "valid") saves.dispatchEvent(new Event("saved"));

  return check;
}

/// Stops using Hermes: the assistant starts on its harness again.
export async function disconnectHermes(): Promise<void> {
  await invoke("hermes_disconnect");
  saves.dispatchEvent(new Event("saved"));
}

/// Calls `listener` after each save of the setting; returns what stops it.
export function onAssistantSaved(listener: () => void): () => void {
  saves.addEventListener("saved", listener);
  return () => saves.removeEventListener("saved", listener);
}
