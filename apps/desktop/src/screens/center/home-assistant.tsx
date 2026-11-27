import { composeMessage } from "../../parts/session/compose.tsx";
import { MACHINES } from "../../stores/machines/machine-store.ts";
import { useEffect, useLayoutEffect, useRef } from "react";
import { makeStartComposer } from "../../parts/session/start-composer.tsx";
import { lastAssistantMachine, startAssistant } from "../../parts/session/agentnew.tsx";
import { ensureAgentHosts, type AgentHost } from "../../stores/agents/agents-state.ts";
import { pillContent } from "../../parts/session/agentpicker.tsx";
import { ASSISTANT_WORDS, assistantHost, resolveAssistant } from "../../stores/agents/assistant-state.ts";
import { loadAssistant, onAssistantSaved } from "../../stores/settings/assistant.ts";
import { FoundSection } from "../project/outside-lists.tsx";
import type { FoundRow } from "../../stores/project/projects-state.ts";
import { PaintedEmpty } from "../../ui/painted-art.tsx";
import { Button } from "../../ui/kit.tsx";
import { reason } from "../../ui/format.ts";
import { addProject, importFromPage } from "../../parts/project/addflow.ts";
import { state } from "../../stores/center/state.ts";
import { go, openScreen } from "../../parts/center/nav.ts";

const SUGGESTION = "Import my projects";

/// Home on first start, before any project: the assistant's composer with
/// one suggestion, then the projects found on the machines.
export function HomeAssistant({ found }: { found: FoundRow[] | null }) {
  const slot = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    const form = composer.form;
    slot.current?.append(form);
    return () => form.remove();
  }, []);

  // Read again when Settings saves the setting.
  useEffect(() => {
    void showHarness();
    return onAssistantSaved(() => void showHarness());
  }, []);

  const suggest = () => {
    composer.setText(SUGGESTION);
    composer.input.focus();
  };

  return <>
    <PaintedEmpty>
      <b>{ASSISTANT_WORDS.title}</b>
      <span>{ASSISTANT_WORDS.line}</span>
      <div className="home-assistant" ref={slot} />
      <Button small className="dchip" onClick={suggest}>{SUGGESTION}</Button>
    </PaintedEmpty>
    {!!found?.length && <div id="sec-found"><FoundSection rows={found} importing={state.importing} error={state.foundError} importOne={importFromPage} add={addProject} /></div>}
  </>;
}

/// One form for Home's life, so its draft stays across redraws. Made with the
/// module: it draws itself at once, which it cannot do inside a React render.
const composer = makeStartComposer({
  presentation: "compact", externalChoices: true,
  // The harness is the assistant setting's, changed in Settings.
  actions: { submit: () => void send(), harness: () => openScreen({ screen: "settings", section: "assistant" }), input: () => composer.update({ invalid: false }) },
  place: () => null,
});
composer.update({ placeholder: ASSISTANT_WORDS.placeholder, words: ASSISTANT_WORDS });
/// True while a send starts the assistant.
let sending = false;

/// The machines that run agents, read once.
async function agentHosts(): Promise<readonly AgentHost[]> {
  return ensureAgentHosts();
}

/// Shows the harness, model and thinking level the assistant starts with.
async function showHarness() {
  const [pick, hosts] = await Promise.all([loadAssistant(), agentHosts()]).catch((e: unknown) => { console.warn("assistant setting:", e); return [null, []] as const; });
  const resolved = pick && resolveAssistant(pick, hosts);
  const agent = resolved?.agent ?? null;
  composer.update({ harnessDisabled: !agent });

  if (!resolved || !agent) {
    composer.setHarness("No agent");
    return;
  }

  const machine = assistantHost(hosts, lastAssistantMachine() || MACHINES.mac?.id || null, agent)?.machine_id ?? null;
  composer.setHarness(pillContent(agent, resolved.model ?? "", resolved.effort, machine));
}

/// Starts the assistant with the task and opens its session.
async function send() {
  const text = composeMessage(composer.input.value).trim();
  if (!text) { composer.update({ invalid: true }); composer.input.focus(); return; }
  if (sending) return;
  sending = true;
  composer.update({ busy: true, invalid: false, error: "" });

  try {
    const [pick, hosts] = await Promise.all([loadAssistant(), agentHosts()]);
    const started = await startAssistant(text, pick, hosts, null);
    composer.setText("");
    go("pane", { agent: started.session, adopt: true });
  } catch (e) {
    composer.update({ error: reason(e) });
  } finally {
    sending = false;
    composer.setBusy(false);
  }
}
