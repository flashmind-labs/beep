import "./dom-env.ts";
import test from "node:test";
import assert from "node:assert/strict";
import { createElement } from "react";
import { FolderChoice, NewAgentNotes } from "../src/parts/session/start-composer-view.tsx";
import { startDom } from "./start-react-env.ts";

const actions = { submit() {}, harness() {} };
const openMachines = () => {}, openAgents = () => {};

test("machine availability notes update without replacing the composer", async () => {
  const env = startDom();
  try {
    const { makeStartComposer } = await import("../src/parts/session/start-composer.tsx");
    const c = makeStartComposer({ place: () => null, actions }); env.dom.window.document.body.append(c.form);
    c.input.value = "Keep this draft"; c.input.focus();
    c.update({ notes: createElement(NewAgentNotes, { waiting: true, none: false, noMachine: false, openAgents, openMachines }) });
    assert.match(c.form.textContent ?? "", /Looking for machines/);
    c.update({ notes: createElement(NewAgentNotes, { waiting: false, none: true, noMachine: false, openAgents, openMachines }) });
    assert.match(c.form.textContent ?? "", /Install a coding agent/);
    c.update({ notes: createElement(NewAgentNotes, { waiting: false, none: false, noMachine: true, openAgents, openMachines }) });
    assert.equal(c.form.querySelector("#agwhy")?.textContent, "Pick a machine to start.");
    assert.equal(c.input.value, "Keep this draft");
    assert.equal(document.activeElement, c.input);
    c.dispose(); c.form.remove();
  } finally { env.close(); }
});

test("new agent choices retain controls across updates", async () => {
  const env = startDom();
  try {
    const { makeStartComposer } = await import("../src/parts/session/start-composer.tsx");
    const c = makeStartComposer({ place: () => null, actions }); env.dom.window.document.body.append(c.form);
    const choices = (machine: string) => ({ machine, folder: createElement(FolderChoice, { checkout: false, label: "Home", detail: "~/work" }) });
    c.update({ choiceContents: choices("First") });
    const folder = c.choice("folder"), machine = c.choice("host");
    assert.ok(folder && machine); machine.focus();
    c.update({ choiceContents: choices("Second") }); c.setBusy(true); c.setBusy(false);
    assert.equal(c.choice("folder"), folder); assert.equal(c.choice("host"), machine);
    assert.equal(document.activeElement, machine); assert.equal(machine.textContent, "Second");
    assert.equal(folder.textContent, "Home ~/work");
    c.dispose(); c.form.remove();
  } finally { env.close(); }
});

test("start composers consume Enter once and preserve Shift+Enter and composition", async () => {
  const env = startDom();
  const { makeStartComposer } = await import("../src/parts/session/start-composer.tsx");
  const composer = makeStartComposer({ place: () => null, actions });
  env.dom.window.document.body.append(composer.form);
  let submitted = 0;
  composer.form.requestSubmit = () => { submitted++; };
  composer.input.value = "Sample task";
  const enter = (options: KeyboardEventInit = {}) => composer.input.dispatchEvent(new env.dom.window.KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true, ...options }));
  try {
    assert.equal(composer.input.title, "Enter starts, Shift+Enter adds a line");
    const intro = composer.form.querySelector(".agc-intro h3"); assert.ok(intro);
    assert.equal(intro.textContent, intro.getAttribute("aria-label"));
    assert.equal(composer.choices.parentElement, composer.form);
    assert.ok(composer.form.matches(".cc-pjc.centered"));
    enter({ shiftKey: true }); enter({ metaKey: true, shiftKey: true }); enter({ isComposing: true });
    const prevent = (event: Event) => event.preventDefault();
    composer.input.addEventListener("keydown", prevent, { once: true });
    enter();
    assert.equal(submitted, 0);
    assert.equal(enter(), false);
    enter({ metaKey: true }); enter({ ctrlKey: true });
    assert.equal(submitted, 3);
    const input = composer.input, choices = composer.choices;
    composer.setPresentation("compact");
    assert.equal(composer.input, input); assert.equal(composer.choices, choices);
    assert.ok(composer.choices.parentElement?.classList.contains("row"));
    assert.equal(composer.input.value, "Sample task");
    enter({ shiftKey: true }); enter({ isComposing: true });
    assert.equal(submitted, 3);
    enter(); assert.equal(submitted, 4);
    // On a phone, Return adds a line.
    Object.defineProperty(env.dom.window, "innerWidth", { value: 390, configurable: true });
    assert.equal(enter(), true); assert.equal(submitted, 4);
  } finally { composer.dispose(); env.close(); }
});

test("project choices keep one owner across busy and layout changes", async () => {
  const env = startDom();
  const { makeStartComposer } = await import("../src/parts/session/start-composer.tsx");
  const composer = makeStartComposer({ place: () => null, presentation: "compact", externalChoices: true, actions });
  env.dom.window.document.body.append(composer.form);
  try {
    composer.setChoices(createElement("button", { "data-test-choice": "machine" }, "Machine"));
    const choice = composer.choices.querySelector("button"); assert.ok(choice);
    composer.setBusy(true); composer.setPresentation("centered"); composer.setBusy(false);
    assert.equal(composer.choices.querySelector("button"), choice);
    assert.equal(composer.choices.parentElement, composer.form);
    composer.setChoices(createElement("button", { "data-test-choice": "machine" }, "Updated"));
    assert.equal(composer.choices.querySelector("button"), choice);
    assert.equal(choice.textContent, "Updated");
  } finally { composer.dispose(); env.close(); }
});
