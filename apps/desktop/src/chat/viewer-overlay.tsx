import { createDocumentScope } from "../ui/document-root.tsx";
import { bar } from "../bar/core.ts";
import { Button } from "../ui/kit.tsx";
import { revealLabel } from "../ui/platform.ts";
import { invoke } from "../ui/tauri.ts";

/// The viewer and its notes, drawn at the end of the element they cover.
export const overlays = createDocumentScope<"viewer" | "notice">();
/// Whether the viewer is open and the element it covers.
export const viewer = { open: false, host: bar as HTMLElement };

let noticeTimer: ReturnType<typeof setTimeout> | undefined;
/// A short note at the bottom of the bar, or over the open viewer; one that names a saved file
/// can show it in Finder.
export function notice(text: string, { path, bad }: { path?: string; bad?: boolean } = {}) {
  clearTimeout(noticeTimer);
  overlays.remove(["notice"]);
  overlays.set([{ slot: "notice", host: viewer.open ? viewer.host : bar, content: <ViewerNotice text={text} path={path ?? null} bad={!!bad} /> }]);
  noticeTimer = setTimeout(() => overlays.remove(["notice"]), 5000);
}

function ViewerNotice({ text, path, bad }: { text: string; path: string | null; bad: boolean }) {
  return <div className={`dsaved${bad ? " bad" : ""}`} id="saved" role="status"><span>{text}</span>{path && <Button onClick={() => void invoke("reveal_file", { path }).catch(() => {})}>{revealLabel()}</Button>}</div>;
}
