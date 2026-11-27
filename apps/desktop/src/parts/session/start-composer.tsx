import type { ReactNode } from "react";
import { flushSync } from "react-dom";
import { createDocumentView } from "../../ui/document-root.tsx";
import { StartComposerView, type StartComposerActions, type StartComposerNodes, type StartComposerState } from "./start-composer-view.tsx";
import { nextSessionIntro } from "../../stores/agents/session-intro.ts";
import { TASK_WORDS } from "../../stores/agents/assistant-state.ts";
import type { MenuPlace } from "./mentionmenu.ts";

type Presentation = "compact" | "centered";

interface Options {
  externalChoices?: boolean;
  presentation?: Presentation;
  /// The ids the new-agent page and its tests use.
  ids?: boolean;
  /// The task field's tooltip.
  title?: string;
  actions: StartComposerActions;
  /// Where the task field's `/` and `@` menus look; null offers none.
  place: () => MenuPlace | null;
}

/// One composer form for the life of its page. The form moves between hosts
/// (a project page and the empty agent view) and keeps its draft, caret and
/// focus, so React owns what is inside it and callers change it through
/// `update` and the setters.
export function makeStartComposer({ externalChoices = false, presentation = "centered", ids = false, title = "Enter starts, Shift+Enter adds a line", actions, place }: Options) {
  const form = document.createElement("form"), choices = document.createElement("span");
  form.className = "cc-pjc";
  choices.className = "choices";
  if (ids) choices.id = "agchoices";
  const nodes: StartComposerNodes = { input: null, machine: null, folder: null, harness: null, pick: null, att: null, box: null, grow: () => {} };
  const root = createDocumentView(form);
  let state: StartComposerState = {
    centered: presentation === "centered", busy: false, welcome: presentation === "centered" ? nextSessionIntro() : "", project: null, draft: null,
    error: "", progress: "", placeholder: "", words: TASK_WORDS, invalid: false, dropping: false, sendDisabled: false, sendTitle: null, stashLocked: false,
    harness: null, harnessDisabled: false, open: null, folderHidden: false, folderTitle: null,
    choiceView: null, machineView: null, choiceContents: null, attachmentView: null, uploadPicker: null, notes: null, footer: null, under: null, extras: null, picker: null,
  };
  const draw = () => {
    form.classList.toggle("centered", state.centered);
    flushSync(() => root.render(<StartComposerView {...{ nodes, choices, externalChoices, ids, title, state, actions, place }} />));
  };
  const update = (patch: Partial<StartComposerState>) => {
    const next = { ...state, ...patch };
    if (Object.keys(patch).every(key => Object.is(next[key as keyof StartComposerState], state[key as keyof StartComposerState]))) return;
    state = next;
    draw();
  };
  const setPresentation = (to: Presentation) => {
    if ((to === "centered") === state.centered) return;
    update({ centered: to === "centered", welcome: to === "centered" ? nextSessionIntro() : state.welcome });
  };

  // The form is this composer's host, so its own submit and file drops are heard on it.
  form.addEventListener("submit", event => { event.preventDefault(); actions.submit(); });
  const take = actions.files;

  if (take) {
    const held = () => state.busy || state.stashLocked;
    form.addEventListener("dragover", event => {
      if (!event.dataTransfer?.types.includes("Files")) return;
      event.preventDefault();
      event.stopPropagation();
      event.dataTransfer.dropEffect = held() ? "none" : "copy";
      if (!held()) update({ dropping: true });
    });
    form.addEventListener("dragleave", event => { if (!(event.relatedTarget instanceof Node && form.contains(event.relatedTarget))) update({ dropping: false }); });
    form.addEventListener("drop", event => {
      update({ dropping: false });
      if (!event.dataTransfer?.files.length) return;
      event.preventDefault();
      event.stopPropagation();
      take([...event.dataTransfer.files]);
    });
  }

  draw();
  const required = <T,>(value: T | null): T => { if (!value) throw new Error("Start composer did not mount"); return value; };
  const view = (key: "choiceView" | "machineView" | "footer" | "picker" | "harness") => (node: ReactNode) => update({ [key]: node });

  return {
    update,
    setPresentation,
    setBusy: (busy: boolean) => update({ busy }),
    setChoices: view("choiceView"),
    setMachine: view("machineView"),
    setFooter: view("footer"),
    setPicker: view("picker"),
    setHarness: view("harness"),
    centered: () => state.centered,
    form, choices,
    get input() { return required(nodes.input); },
    harness: required(nodes.harness),
    pick: required(nodes.pick),
    att: required(nodes.att),
    box: required(nodes.box),
    /// The choice pills of the new-agent form, for placing their menus.
    choice: (which: "host" | "folder") => (which === "host" ? nodes.machine : nodes.folder),
    /// Puts `text` in the task field, as sending or a fill from elsewhere does.
    setText: (text: string) => { required(nodes.input).value = text; nodes.grow(); },
    /// Fits the task field to its text; a no-op until the form is on the page.
    grow: () => nodes.grow(),
    dispose: () => root.unmount(),
  };
}

export type StartComposer = ReturnType<typeof makeStartComposer>;
