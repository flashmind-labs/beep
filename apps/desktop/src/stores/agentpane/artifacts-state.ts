// The Artifacts tab of a coding agent session: the pages and documents the
// agent made, read from its log: the files its calls wrote or published, and
// the ones its commands made, which the node names in a note at the turn's
// end, a `(made <path>)` line each.

import { imageLines, parseTool } from "../agents/agentlog.ts";
import { baseName, extension, mediaKind, resolvePath } from "./media-state.ts";

/// How the tab shows an artifact.
type ArtifactKind = "html" | "md" | "svg" | "image" | "pdf";

const KINDS: Record<string, ArtifactKind> = {
  html: "html", htm: "html",
  md: "md", markdown: "md",
  svg: "svg",
  png: "image", jpg: "image", jpeg: "image", gif: "image", webp: "image",
  pdf: "pdf",
};

/// What a kind is called in the list.
export const KIND_NAMES: Record<ArtifactKind, string> = { html: "HTML", md: "Markdown", svg: "SVG", image: "Image", pdf: "PDF" };

/// What the frame of an HTML artifact may do: run its scripts, nothing more.
export const FRAME_SANDBOX = "allow-scripts";

/// One artifact: a file the agent published or wrote, as its newest call
/// left it.
export interface Artifact {
  /// Where the node reads it: absolute, or `~/` for its home.
  path: string;
  name: string;
  kind: ArtifactKind;
  /// The claude.ai link of the newest call that published it.
  url: string | null;
  /// Some call published it with the Artifact tool.
  published: boolean;
  /// A command made it, as the node noted at a turn's end.
  made: boolean;
  /// The newest call's place in the session, its status, and how many
  /// calls wrote it.
  at: number;
  status: string;
  versions: number;
  /// The place of the message that opened the newest call's turn, and
  /// which of the messages shown that is, from 1; 0 before the first.
  turn: number;
  turnNo: number;
}

/// What the tab shows `path` as, or null for any other file.
function artifactKind(path: string): ArtifactKind | null {
  return KINDS[extension(path)] ?? null;
}

/// A file one call wrote or published.
export interface Written {
  path: string;
  url: string | null;
  published: boolean;
}

const TITLE = /^(Artifact|Write|Edit|MultiEdit|Create|Update)\b:?\s+(\S.*)$/;
const CHANGED = /^\(changed (.+)\)$/;
const MADE = /^\(made (.+)\)$/;
const LINK = /https:\/\/claude\.ai\/[^\s"'<>)\]`]*artifact\/[^\s"'<>)\]`]+/;

/// The first claude.ai artifact link in `text`, without the full stop
/// after it.
function link(text: string) {
  return LINK.exec(text)?.[0].replace(/[.,;:]+$/, "") ?? null;
}

/// The artifact files a tool entry's text names, each once, and its
/// status: a call that failed, or has not finished, wrote nothing yet.
export function toolArtifacts(text: string, cwd: string): { status: string; files: Written[] } {
  const { title, status, output } = parseTool(imageLines(text).text);
  if (status === "failed" || status === "pending" || status === "in_progress") return { status, files: [] };
  const files: Written[] = [];
  const add = (raw: string, url: string | null, published: boolean) => {
    const name = raw.trim();
    if (!name || !artifactKind(name)) return;
    const path = resolvePath(cwd, name);
    const had = files.find(f => f.path === path);
    if (had) { had.published ||= published; had.url ??= url; return; }
    files.push({ path, url, published });
  };
  const named = TITLE.exec(title.split("\n")[0]);

  if (named) {
    const published = named[1] === "Artifact";
    add(named[2], published ? link(output) : null, published);
  }

  for (const line of output.split("\n")) {
    const m = CHANGED.exec(line.trim());
    if (m) add(m[1], null, false);
  }

  return { status, files };
}

/// The artifact files a note of the node names as made by the turn's
/// commands, each resolved against `cwd`; none for any other note.
function madeArtifacts(text: string, cwd: string): string[] {
  return madeNames(text).filter(name => artifactKind(name)).map(name => resolvePath(cwd, name));
}

/// The paths a note of `(made <path>)` lines names, as written; none for
/// any other note.
function madeNames(text: string) {
  const lines = text.split("\n").map(line => MADE.exec(line.trim()));
  return lines.every(m => m) ? lines.map(m => m![1]) : [];
}

/// What the log groups made files by, in the order it shows them.
type MadeGroup = "images" | "documents" | "pdfs" | "pages" | "models" | "other";

const GROUP_ORDER: readonly MadeGroup[] = ["images", "documents", "pdfs", "pages", "models", "other"];

/// A group's label over its names, and how its count reads for one and many.
const GROUP_NAMES: Record<MadeGroup, { label: string; one: string; many: string }> = {
  images: { label: "Images", one: "image", many: "images" },
  documents: { label: "Documents", one: "document", many: "documents" },
  pdfs: { label: "PDFs", one: "PDF", many: "PDFs" },
  pages: { label: "Web pages", one: "web page", many: "web pages" },
  models: { label: "3D models", one: "3D model", many: "3D models" },
  other: { label: "Other", one: "other file", many: "other files" },
};

const BY_KIND: Record<ArtifactKind, MadeGroup> = { image: "images", svg: "images", md: "documents", pdf: "pdfs", html: "pages" };

/// The group a made file goes in.
export function madeGroup(path: string): MadeGroup {
  const kind = artifactKind(path);
  if (kind) return BY_KIND[kind];
  return mediaKind(path) === "model" ? "models" : "other";
}

/// One made file: where it is, resolved against the session's folder, and its name.
export interface MadeFile {
  path: string;
  name: string;
}

/// The files a note of the node says the turn made, grouped for the log.
export interface MadeFiles {
  files: MadeFile[];
  /// The groups that have files, in order, each with its label.
  groups: { group: MadeGroup; label: string; files: MadeFile[] }[];
  /// "8 images, 3 documents".
  counts: string;
}

/// The files a note of `(made <path>)` lines names, each once and grouped;
/// null for any other note.
export function madeFiles(text: string, cwd: string): MadeFiles | null {
  const files: MadeFile[] = [];

  for (const name of madeNames(text)) {
    const path = resolvePath(cwd, name);
    if (!files.some(f => f.path === path)) files.push({ path, name: baseName(path) });
  }

  if (!files.length) return null;
  const groups = GROUP_ORDER
    .map(group => ({ group, label: GROUP_NAMES[group].label, files: files.filter(f => madeGroup(f.path) === group) }))
    .filter(g => g.files.length);
  const counts = groups.map(g => `${g.files.length} ${g.files.length === 1 ? GROUP_NAMES[g.group].one : GROUP_NAMES[g.group].many}`).join(", ");
  return { files, groups, counts };
}

/// One entry of the log, as the session view has it.
export interface LogEntry {
  kind: string;
  text: string;
}

/// Every artifact among `entries`, the newest first; `earlier` is the place of
/// the first entry in the session.
export function collectArtifacts(entries: LogEntry[], earlier: number, cwd: string): Artifact[] {
  const byPath = new Map<string, Artifact>();
  let turn = earlier, turnNo = 0;
  // The files this turn's calls wrote, which its note does not name again.
  let written = new Set<string>();
  entries.forEach((en, i) => {
    const at = earlier + i;
    if (en.kind === "user") { turn = at; turnNo++; written = new Set(); return; }
    let status = "completed", files: Written[] = [], made = false;

    if (en.kind === "tool") ({ status, files } = toolArtifacts(en.text, cwd));
    else if (en.kind === "other") {
      files = madeArtifacts(en.text, cwd).filter(path => !written.has(path)).map(path => ({ path, url: null, published: false }));
      made = true;
    }

    for (const f of files) {
      const had = byPath.get(f.path);
      if (!made) written.add(f.path);
      byPath.set(f.path, {
        path: f.path,
        name: baseName(f.path),
        kind: artifactKind(f.path)!,
        url: f.url ?? had?.url ?? null,
        published: f.published || !!had?.published,
        made: made || !!had?.made,
        at,
        status,
        versions: (had?.versions ?? 0) + 1,
        turn,
        turnNo,
      });
    }
  });
  return [...byPath.values()]
    .filter(a => a.published || a.made || !readPlace(cwd, a.path).inside)
    .sort((a, b) => b.at - a.at);
}

/// The listed artifacts the machine no longer has, by path, from its
/// answer for `paths` in order; none when it could not say.
export function goneArtifacts(paths: string[], present: boolean[] | null): Set<string> {
  if (!present || present.length !== paths.length) return new Set();
  return new Set(paths.filter((_, i) => !present[i]));
}

/// The message for an artifact the machine no longer has.
export function goneMessage(name: string, machine: string) {
  return `${name} is not on ${machine} any more.`;
}

/// What changes when the agent writes the artifact again, so the tab
/// reads it again.
export function artifactVersion(a: Artifact) {
  return `${a.at}:${a.status}:${a.url ?? ""}`;
}

/// Where the node reads `path` from: the session's folder and the path in
/// it when it is inside, else its own folder. `inside` says which.
export function readPlace(cwd: string, path: string) {
  const root = cwd.replace(/\/+$/, "");
  if (root && path.startsWith(`${root}/`)) return { root, path: path.slice(root.length + 1), inside: true };
  const cut = path.lastIndexOf("/");
  return { root: cut > 0 ? path.slice(0, cut) : "/", path: path.slice(cut + 1), inside: false };
}

/// `path` relative to the session's folder when inside it, for the Files tab.
export function relativeTo(cwd: string, path: string) {
  const place = readPlace(cwd, path);
  return place.inside ? place.path : null;
}
