import { createElement, type ReactNode } from "react";
import { flushSync } from "react-dom";
import { createDocumentView } from "../ui/document-root.tsx";
import { StartComposerView, type StartComposerNodes, type ChoiceContents } from "./start-composer-view.tsx";
import { nextSessionIntro } from "./session-intro.ts";

const FIELD_MAX = 200;
type Presentation = "compact" | "centered";

export function makeStartComposer({ projectPicker = false, externalChoices = false, presentation = "centered" }: { projectPicker?: boolean; externalChoices?: boolean; presentation?: Presentation } = {}) {
  const form = document.createElement("form"), choices = document.createElement("span");
  form.className = "cc-pjc";
  choices.className = "choices";
  const nodes: StartComposerNodes = { input: null, project: null, machine: null, folder: null, extra: null, harness: null, send: null, pick: null, att: null, err: null, box: null };
  const root = createDocumentView(form);
  let choiceView: ReactNode = null, attachmentView: ReactNode = null, uploadPicker: ReactNode = null, notes: ReactNode = null, footer: ReactNode = null, under: ReactNode = null, extras: ReactNode = null, picker: ReactNode = null, harnessView: ReactNode = null;
  let choiceContents: ChoiceContents | null = null;
  let current: Presentation = presentation, busy = false, welcome = presentation === "centered" ? nextSessionIntro() : "";
  const grow = () => {
    const input = nodes.input;
    if (!input?.isConnected) return;
    input.style.height = "auto";
    input.style.height = `${Math.min(input.scrollHeight, FIELD_MAX)}px`;
    input.style.overflowY = input.scrollHeight > FIELD_MAX ? "auto" : "hidden";
  };
  const draw = () => {
    form.classList.toggle("centered", current === "centered");
    flushSync(() => root.render(createElement(StartComposerView, { nodes, choices, projectPicker, externalChoices, choiceView, choiceContents, attachmentView, uploadPicker, notes, footer, under, extras, picker, harnessView, centered: current === "centered", busy, welcome, grow, submit: () => form.requestSubmit() })));
  };
  const setPresentation = (to: Presentation) => {
    if (to === current) return;
    current = to;
    if (to === "centered") welcome = nextSessionIntro();
    draw();
  };
  const setBusy = (value: boolean) => { if (busy !== value) { busy = value; draw(); } };
  draw();
  const required = <T,>(value: T | null): T => { if (!value) throw new Error("Start composer did not mount"); return value; };
  const project = nodes.project ?? document.createElement("button");
  if (!projectPicker) { project.type = "button"; project.title = "The project this agent works on"; }
  const machine = nodes.machine ?? document.createElement("button"), folder = nodes.folder ?? document.createElement("button"), extra = nodes.extra ?? document.createElement("span");
  const setChoices = (view: ReactNode) => { choiceView = view; draw(); };
  const setChoiceContents = (contents: ChoiceContents) => { choiceContents = contents; draw(); };
  const setAttachments = (view: ReactNode) => { attachmentView = view; draw(); };
  const setUploadPicker = (view: ReactNode) => { uploadPicker = view; draw(); };
  const setNotes = (view: ReactNode) => { notes = view; draw(); };
  const setFooter = (view: ReactNode) => { footer = view; draw(); };
  const setUnder = (view: ReactNode) => { under = view; draw(); };
  const setExtras = (view: ReactNode) => { extras = view; draw(); };
  const setPicker = (view: ReactNode) => { picker = view; draw(); };
  const setHarness = (view: ReactNode) => { harnessView = view; draw(); };
  return { setPresentation, setBusy, setChoices, setChoiceContents, setAttachments, setUploadPicker, setNotes, setFooter, setUnder, setExtras, setPicker, setHarness, centered: () => current === "centered", form, input: required(nodes.input), project, machine, folder, extra, choices, harness: required(nodes.harness), send: required(nodes.send), pick: required(nodes.pick), att: required(nodes.att), err: required(nodes.err), box: required(nodes.box), grow, dispose: () => root.unmount() };
}
