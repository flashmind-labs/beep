import type { ReactNode } from "react";
import { createDocumentScope } from "../ui/document-root.tsx";
import type { DiffOwner, DiffSlot } from "./diff.ts";

let next = 0;
/// The diffs' portals. Their own scope keeps the bar window from loading the command center's root.
const scope = createDocumentScope<`${number}:${DiffSlot}`>();

/// Each retained diff owns its slots; dropping it removes its portals together.
export function windowDiffOwner(): DiffOwner {
  const id = ++next, slots = new Set<DiffSlot>();
  return {
    set(slot: DiffSlot, content: ReactNode) {
      slots.add(slot);
      scope.set([{ slot: `${id}:${slot}`, host: null, content }]);
    },
    dispose() { scope.remove([...slots].map(slot => `${id}:${slot}` as const)); slots.clear(); },
  };
}
