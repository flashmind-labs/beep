import type { ReactNode } from "react";
import { removeWindowParts, showWindowParts } from "../center/window-root.ts";
import type { DiffOwner, DiffSlot } from "./diff.ts";

let next = 0;

/// Each retained diff owns its slots; dropping it removes its portals together.
export function windowDiffOwner(): DiffOwner {
  const id = ++next, slots = new Set<DiffSlot>();
  return {
    set(slot: DiffSlot, content: ReactNode) {
      slots.add(slot);
      showWindowParts([{ part: `diff:${id}:${slot}`, host: null, content }]);
    },
    dispose() { removeWindowParts([...slots].map(slot => `diff:${id}:${slot}` as const)); slots.clear(); },
  };
}
