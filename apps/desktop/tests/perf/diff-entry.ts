import { diffView as createDiffView, type DiffSource } from "../../src/review/diff.tsx";
import { windowDiffOwner } from "../../src/review/diff-owner.ts";

export function diffView(source: Omit<DiffSource, "owner">) {
  return createDiffView({ ...source, owner: windowDiffOwner() });
}
