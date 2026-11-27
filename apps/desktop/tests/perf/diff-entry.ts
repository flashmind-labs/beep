import { diffView as createDiffView, type DiffSource } from "../../src/parts/review/diff.tsx";
import { windowDiffOwner } from "../../src/parts/review/diff-owner.ts";

export function diffView(source: Omit<DiffSource, "owner">) {
  return createDiffView({ ...source, owner: windowDiffOwner() });
}
