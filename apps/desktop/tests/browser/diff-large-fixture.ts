// A synthetic branch diff shaped like a large real one: about 600 files and
// 40k changed lines, most files small, a few past the fold limit. Paths and
// text are made up.
import { diffView } from "../../src/parts/review/diff.tsx";
import { windowDiffOwner } from "../../src/parts/review/diff-owner.ts";
import type { DiffFile, DiffLine, DiffView } from "../../src/stores/review/diff-state.ts";

function random(seed: number) {
  let state = seed >>> 0;
  return () => (state = (state * 1664525 + 1013904223) >>> 0) / 2 ** 32;
}

/// Changed lines per file, sampled to match a median of 40, a 90th
/// percentile near 140 and a handful over 400.
function size(next: () => number) {
  const p = next();
  if (p < 0.26) return 2 + Math.floor(next() * 18);
  if (p < 0.5) return 20 + Math.floor(next() * 20);
  if (p < 0.9) return 40 + Math.floor(next() * 100);
  if (p < 0.983) return 140 + Math.floor(next() * 260);
  return 400 + Math.floor(next() * 440);
}

function file(index: number, next: () => number, variant: number): DiffFile {
  const changed = size(next);
  const path = `pkg-${index % 7}/src/area-${index % 13}/part-${index % 5}/module-${index}.ts`;
  const hunks = [];
  let left = changed, start = 10, additions = 0, deletions = 0;

  while (left > 0) {
    const take = Math.min(left, 12 + Math.floor(next() * 30));
    const lines: DiffLine[] = [];
    for (let i = 0; i < 3; i++) lines.push({ k: " ", t: `  const context${i} = compute(${index}, ${i});` });
    let oldLines = 3, newLines = 3;

    for (let i = 0; i < take; i++) {
      const add = next() < 0.65;
      lines.push({ k: add ? "+" : "-", t: `  ${add ? "return" : "yield"} transform(value${i}, "${"sample ".repeat(1 + Math.floor(next() * 8))}", ${variant});` });
      if (add) { additions++; newLines++; } else { deletions++; oldLines++; }
    }

    for (let i = 0; i < 3; i++) lines.push({ k: " ", t: `  // trailing ${i}` });
    hunks.push({ old_start: start, old_lines: oldLines + 3, new_start: start, new_lines: newLines + 3, section: `function block${start}()`, lines });
    left -= take;
    start += take + 40;
  }

  return { path, old_path: null, status: "modified", additions, deletions, hunks, left_out: null };
}

export function largeDiff(variant = 0, count = 600): DiffView {
  const next = random(552);
  const files = Array.from({ length: count }, (_, i) => file(i, next, i < 3 ? variant : 0));
  return {
    root: "/srv/project", base: "main", merge_base: "", working_tree: false, more: 0, files,
    additions: files.reduce((n, f) => n + f.additions, 0), deletions: files.reduce((n, f) => n + f.deletions, 0),
  };
}

let current: ReturnType<typeof diffView> | null = null;
let view = largeDiff();
/// A copy of the view made ahead, as the app gets one fresh from each read.
let ready: DiffView | null = null;
const read = () => { const got = ready ?? structuredClone(view); ready = null; return got; };

Reflect.set(window, "openLargeDiff", (readOnly = false) => {
  current?.dispose();
  const v = diffView({
    owner: windowDiffOwner(), readOnly,
    load: async () => read(),
    review: { key: "large-diff", heading: () => "Review", send: async () => "" },
  });
  document.querySelector("#host")?.replaceChildren(v.el);
  current = v;
});
Reflect.set(window, "prepareLargeDiff", (variant: number) => { view = largeDiff(variant); ready = structuredClone(view); });
Reflect.set(window, "updateLargeDiff", () => current?.reload());
Reflect.set(window, "largeDiffStats", () => {
  const files = view.files;
  return { files: files.length, lines: files.reduce((n, f) => n + f.additions + f.deletions, 0) };
});
