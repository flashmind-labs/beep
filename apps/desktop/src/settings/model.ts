import { flushSync } from "react-dom";
import { type BarPageSlot, openBarPage } from "../bar/bar-app.tsx";
import { S } from "../bar/core.ts";
import { hintModel, refreshHints } from "../bar/hints.ts";
import { cap } from "../chat/tools.ts";
import { ICON } from "../ui/icons.tsx";
import { invoke } from "../ui/tauri.ts";

// ---------- Model (admins) ----------

/// How long the model reasons before it answers (ReasoningLevel).
export type Thinking = "off" | "low" | "medium" | "high";

/// A model the caller can pick (ModelChoice).
export interface ModelChoice {
  id: string;
  name: string;
  /// USD per million tokens, when the provider lists it.
  input_price: number | null;
  output_price: number | null;
  sees_images: boolean;
  default: boolean;
}

/// The models the caller can pick from and the one their turns use (Models).
export interface Models {
  /// Empty on servers from before every user could pick.
  choices: ModelChoice[];
  selected: string;
  thinking: Thinking;
  auto: boolean;
  auto_available: boolean;
}

/// Whether agents are on for everyone, and the agents set up (Agents).
interface Agents {
  enabled: boolean;
  agents: { id: string; name: string; when: string; model: string; thinking: Thinking }[];
}

/// The caller's model settings.
export const MODELS: { info: Models | null; sel: number; agents: boolean; list: string[] } = { info: null, sel: 0, agents: false, list: [] };
export const THINKING: Thinking[] = ["off", "low", "medium", "high"];

/// Whether the server offers model choices (every user once it lists them).
export function canPickModel() { return !!MODELS.info?.choices.length; }
export async function loadModels() {
  try {
    MODELS.info = await invoke<Models>("get_models");
  } catch {
    return;
  }

  refreshHints();
  if (!canPickModel()) return;

  try {
    takeAgents(await invoke<Agents>("get_agents"));
  } catch {
    return;
  }

  if (S.view === "model") drawModel();
}
function takeAgents(agents: Agents) {
  MODELS.agents = agents.enabled;
  MODELS.list = agents.agents.map(a => a.name);
}
export function modelName(id: string) { return MODELS.info?.choices.find(c => c.id === id)?.name ?? id.split(/[:/]/).pop()!; }
export function modelPrice(c: ModelChoice) {
  const usd = (n: number) => "$" + (Math.round(n * 100) / 100);
  const price = c.input_price != null && c.output_price != null ? `${usd(c.input_price)} in, ${usd(c.output_price)} out per M tokens` : "No price listed";
  return c.sees_images ? price : price + ", text only";
}
/// The hint row's chip: the model and thinking every turn uses, or Auto.
export function modelChipLabel(): string | null {
  const m = MODELS.info;
  if (!m?.choices.length) return null;
  return m.auto ? "Auto" : `${modelName(m.selected)} · ${cap(m.thinking)}`;
}

export function renderModel() {
  const m = MODELS.info;
  if (!m?.choices.length) return;
  S.view = "model";
  MODELS.sel = Math.max(0, m.choices.findIndex(c => c.id === m.selected));
  modelError = "";
  drawModel();
}
let modelPage: BarPageSlot | null = null;
let modelError = "";

function drawModel() {
  const m = MODELS.info;
  if (!m) return;
  const page = modelPage ??= openBarPage(() => { modelPage = null; });
  flushSync(() => page.show({ view: "model", props: {
    model: m, selected: MODELS.sel, agents: MODELS.agents, agentNames: MODELS.list,
    error: modelError, hints: hintModel([["↑↓", "move"], [ICON.ret, "use"], ["esc", "back"]]), thinkingLevels: THINKING, price: modelPrice, capitalize: cap,
    pick: (index: number) => { MODELS.sel = index; void pickModel(); },
    setThinking: (level: Thinking) => { void changeModel("set_thinking", { level }); },
    setAuto: () => { void changeModel("set_auto_model", { enabled: !m.auto }); },
    setAgents: () => { void change("set_agents", { enabled: !MODELS.agents }, takeAgents); },
  } }));
}
/// Runs a change the server answers with `T`, which `take` stores; an error
/// shows under the form.
async function change<T>(command: string, args: Record<string, unknown>, take: (answer: T) => void) {
  try {
    take(await invoke<T>(command, args));
  } catch (e) {
    modelError = String(e);
    if (S.view === "model") drawModel();
    return;
  }

  modelError = "";
  if (S.view === "model") drawModel();
}
function changeModel(command: string, args: Record<string, unknown>) {
  return change<Models>(command, args, m => { MODELS.info = m; });
}
const pickModel = () => changeModel("set_model", { id: MODELS.info!.choices[MODELS.sel].id });
export function modelKeys(e: KeyboardEvent) {
  const m = MODELS.info!;
  if (MODELS.agents && m.auto_available && m.auto) return;
  const n = m.choices.length;

  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    MODELS.sel = (MODELS.sel + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
    modelError = "";
    return drawModel();
  }

  if (e.key === "Enter") { e.preventDefault(); return pickModel(); }
}
