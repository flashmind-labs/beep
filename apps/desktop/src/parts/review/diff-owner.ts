import type { ReactNode } from "react";
import { createDocumentScope } from "../../ui/document-root.tsx";
import type { DiffOwner } from "./diff.tsx";

let next = 0;
/// The diffs' portals. Their own scope keeps the bar window from loading the command center's root.
const scope = createDocumentScope<`${number}:viewer`>();

/// Each retained diff owns its portal; dropping it removes the portal.
export function windowDiffOwner(): DiffOwner {
  const slot = `${++next}:viewer` as const;
  let drawn = false;
  return {
    set(content: ReactNode) {
      drawn = true;
      scope.set([{ slot, host: null, content }]);
    },
    dispose() {
      if (drawn) scope.remove([slot]);
      drawn = false;
    },
  };
}
