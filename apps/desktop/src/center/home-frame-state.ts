// The kept Home frame: the copy of the window that is saved, without what must not show stale.

/// What Home's window drew, as `keep_home_frame` takes it.
export interface HomeFrame {
  skin: string | null;
  html: string;
}

/// `win` as a frame: an inert copy without what must not show stale or cannot work in a copy.
export function frameOf(win: Element, skin: string | null): HomeFrame {
  const copy = win.cloneNode(true);
  if (!(copy instanceof Element)) throw new Error("The window did not copy");

  copy.classList.add("kept");
  copy.setAttribute("inert", "");
  copy.setAttribute("aria-hidden", "true");

  // The real pane box holds the page's `#bar`; the copy must not hold a second one.
  copy.querySelector("#pane-box")?.replaceChildren();
  for (const el of copy.querySelectorAll("iframe, video, audio, canvas")) el.remove();

  // A draft typed in Home's composer stays out of the file.
  for (const el of copy.querySelectorAll("input, textarea")) {
    if (el instanceof HTMLInputElement) {
      el.value = "";
      el.removeAttribute("value");
    } else if (el instanceof HTMLTextAreaElement) {
      el.value = "";
      el.textContent = "";
    }
  }

  return { skin, html: copy.outerHTML };
}
