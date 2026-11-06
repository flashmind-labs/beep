// How a picture in a session's log is drawn, apart from the page.
import { esc } from "../ui/format.ts";
import { ICON } from "../ui/icons.ts";

/// A picture as read from its machine: the data URL, a promise while it is
/// read, null when the machine no longer has it, nothing when not asked yet.
export type PicRead = Promise<string | null> | string | null | undefined;

/// A session picture that is not here: its icon and name.
export const gonePic = (label: string) => `<span class="pic none" title="${esc(label)}">${ICON.image}${esc(label)}</span>`;

/// The tile of a picture a log entry names `n`: `sent` when this window sent
/// it, else one read from the session's machine by its key, `image-<key>`.
/// The tile of a picture read by key has no `src` whether or not it is here
/// yet (`fillAgentPics` sets it), so the html a log row is drawn from stays
/// the same as it loads and the row is not drawn again.
export function picTile(n: string, sent: string | undefined, thumb: PicRead) {
  const key = /^image-([0-9a-f]{16})$/.exec(n)?.[1], label = key ? "Image" : n;
  const named = `${key ? ` data-key="${key}"` : ""} data-name="${esc(n)}" alt="${esc(label)}" title="${esc(label)}"`;
  if (sent) return `<img class="pic" src="${esc(sent)}"${named}>`;
  if (key && thumb !== null) return `<img class="pic"${named}>`;
  return gonePic(label);
}

/// How long until a picture that failed to load, for a reason other than
/// being gone, is asked for again, after `fails` failures in a row.
export const retryIn = (fails: number) => Math.min(60_000, 2_000 * 2 ** Math.max(0, fails - 1));
