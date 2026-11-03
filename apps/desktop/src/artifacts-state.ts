// The Artifacts tab of a coding agent session: the pages and documents
// the agent made, read from its log. Claude Code publishes one with its
// Artifact tool (`Artifact /srv/project/report.html`, its result giving
// the claude.ai link); any agent writes .html, .md, .svg or image files
// (`Write: …`, `Edit …`, or Codex's `(changed …)` lines). A file it only
// wrote counts when it is outside the session's folder: one inside, such
// as a README.md it edited, is a change to the repo and shows in Files and
// Changes instead. The tab renders
// the file the agent wrote; the claude.ai link only opens in the browser,
// as it needs the user's own claude.ai login.

import { imageLines, parseTool } from "./agentlog.ts";
import { baseName, extension, resolvePath } from "./media-state.ts";

/// How the tab shows an artifact.
export type ArtifactKind = "html" | "md" | "svg" | "image";

const KINDS: Record<string, ArtifactKind> = {
  html: "html", htm: "html",
  md: "md", markdown: "md",
  svg: "svg",
  png: "image", jpg: "image", jpeg: "image", gif: "image", webp: "image",
};

/// What a kind is called in the list.
export const KIND_NAMES: Record<ArtifactKind, string> = { html: "HTML", md: "Markdown", svg: "SVG", image: "Image" };

/// The largest artifact the tab reads.
export const MAX_ARTIFACT = 5 * 1024 * 1024;

/// What the frame of an HTML artifact may do: run its scripts, nothing
/// more. Without `allow-same-origin` it has an origin of its own, so it
/// cannot reach the window, its storage or Beep's calls; without
/// `allow-top-navigation` it cannot move the window away.
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
export function artifactKind(path: string): ArtifactKind | null {
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

/// One entry of the log, as the session view has it.
export interface LogEntry {
  kind: string;
  text: string;
}

/// Every artifact among `entries`, the newest first; `earlier` is the
/// place of the first entry in the session. A file written again is one
/// artifact, as its newest call left it. A file never published that sits
/// inside the session's folder is a repo file, not an artifact.
export function collectArtifacts(entries: LogEntry[], earlier: number, cwd: string): Artifact[] {
  const byPath = new Map<string, Artifact>();
  let turn = earlier, turnNo = 0;
  entries.forEach((en, i) => {
    const at = earlier + i;
    if (en.kind === "user") { turn = at; turnNo++; return; }
    if (en.kind !== "tool") return;
    const { status, files } = toolArtifacts(en.text, cwd);
    for (const f of files) {
      const had = byPath.get(f.path);
      byPath.set(f.path, {
        path: f.path,
        name: baseName(f.path),
        kind: artifactKind(f.path)!,
        url: f.url ?? had?.url ?? null,
        published: f.published || !!had?.published,
        at,
        status,
        versions: (had?.versions ?? 0) + 1,
        turn,
        turnNo,
      });
    }
  });
  return [...byPath.values()]
    .filter(a => a.published || !readPlace(cwd, a.path).inside)
    .sort((a, b) => b.at - a.at);
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

const attr = (s: string) => s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;");

/// The frame an HTML artifact shows in: the page served from its own
/// origin (`src`), sandboxed to run scripts and nothing more, sent no
/// referrer.
export function frameHtml(src: string, title: string) {
  return `<iframe class="agartframe" sandbox="${FRAME_SANDBOX}" referrerpolicy="no-referrer" title="${attr(title)}" src="${attr(src)}"></iframe>`;
}
