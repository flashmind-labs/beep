// The machine, harness, model and thinking picker opened from a button, as
// Review with agent in a diff and on a pull request, and Work on it on an
// issue, show it: it opens under the button, closes on Esc or a click
// outside, and its last step closes it and starts.
import type { ReactElement } from "react";
import type { Picked } from "./agentpicker.tsx";
import { type Allowed, keepEffort, type MachineRow } from "../../stores/agents/agentpick.ts";

// The picker module pulls in the whole session view, so it loads the first
// time a picker opens.
const picker = () => import("./agentpicker.tsx");

export interface PickPopoverOpts {
  picked: Picked;
  /// The machine models are listed on, with its harnesses; null for none.
  machine: () => { id: string; name: string; agents: readonly string[] } | null;
  /// The harnesses any machine has.
  everywhere: () => readonly string[];
  /// The harnesses the project allows; null for every one.
  allowed: () => Allowed;
  /// The Machine step's rows and what picking one does; null where the machine is fixed.
  machines: () => { rows: readonly MachineRow[]; pick: (id: string) => void } | null;
  /// Sets the pick's defaults as the picker opens; false keeps it shut.
  opening: () => boolean;
  /// The last step was picked and the picker closed.
  start: () => void;
  /// Draws the button again; `sync` is false while React is committing.
  draw: (sync: boolean) => void;
}

export interface PickPopover {
  readonly open: boolean;
  readonly view: ReactElement | null;
  /// The elements the view renders: the box clicks inside keep the picker
  /// open, its button and the slot the picker draws into.
  setBox: (box: HTMLElement | null) => void;
  setButton: (button: HTMLButtonElement | null) => void;
  setSlot: (slot: HTMLDivElement | null) => void;
  toggle: () => Promise<void>;
  close: () => void;
  /// Draws the open picker again, as its machine's harnesses change.
  redraw: () => Promise<void>;
}

export function pickPopover(o: PickPopoverOpts): PickPopover {
  let box: HTMLElement | null = null;
  let button: HTMLButtonElement | null = null;
  let slot: HTMLDivElement | null = null;
  let view: ReactElement | null = null;
  let open = false;
  /// Set while the box unmounts, when React cannot flush.
  let committing = false;
  let closeView: ((el: HTMLElement) => void) | null = null;
  const connected = () => !!box?.isConnected;

  function close() {
    open = false;
    if (slot) closeView?.(slot);
    document.removeEventListener("pointerdown", outside, true);
    document.removeEventListener("keydown", onEsc, true);
  }

  const outside = (e: Event) => {
    if (!connected() || !(e.target instanceof Node) || !box?.contains(e.target)) { close(); o.draw(true); }
  };
  const onEsc = (e: KeyboardEvent) => {
    if (e.key !== "Escape") return;
    // A box taken off the page lets the key through.
    if (!connected()) { close(); o.draw(true); return; }
    e.preventDefault();
    e.stopPropagation();
    close();
    o.draw(true);
    button?.focus();
  };

  /// The Machine step, its pick drawing the picker for the machine.
  function machinesStep() {
    const step = o.machines();
    return step && { rows: step.rows, pick: (id: string) => { step.pick(id); void redraw(); } };
  }

  async function redraw() {
    if (open && !connected()) { close(); return; }
    const machine = o.machine();
    if (!open || !machine || !slot || !button || !connected()) return;
    const { drawPicker, focusSearch, closedPicker } = await picker();
    closeView = closedPicker;
    if (!open || !slot || !button || !connected()) return;
    const currentSlot = slot, currentButton = button;
    const focused = currentSlot.contains(document.activeElement);
    drawPicker(currentSlot, {
      render: next => { view = next; o.draw(!committing); },
      picked: o.picked,
      offered: machine.agents,
      everywhere: o.everywhere(),
      allowed: o.allowed(),
      machine: { id: machine.id, name: machine.name },
      noMachine: "",
      steps: { machines: machinesStep() },
      anchor: () => currentButton,
      changed: what => {
        if (what === "done") { close(); o.draw(true); o.start(); return; }
        if (what === "agent") o.picked.effort = keepEffort(o.picked.agent, o.picked.effort);
        if (what !== "models") void redraw();
        if (what === "agent") focusSearch(currentSlot);
      },
    });
    if (focused) focusSearch(currentSlot);
  }

  async function toggle() {
    if (!button || button.disabled) return;
    if (open) { close(); return o.draw(true); }
    const p = await picker();
    if (open || !connected() || !o.opening()) return;
    o.picked.query = "";
    open = true;
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", onEsc, true);
    o.draw(true);
    await redraw();
    if (slot) p.focusSearch(slot);
  }

  return {
    get open() { return open; },
    get view() { return view; },
    setBox: value => {
      box = value;
      // The box went with the picker open.
      if (!value && open) { committing = true; close(); committing = false; o.draw(false); }
    },
    setButton: value => { button = value; },
    setSlot: value => { slot = value; },
    toggle,
    close,
    redraw,
  };
}
