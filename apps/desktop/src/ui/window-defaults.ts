// Document behavior every window's entry imports first.

// Looping CSS animations take their phase from the document's clock instead of
// from when their element appeared, so a redraw never restarts them. Only
// spinners for the user's own waits and the open transcript still loop, so this
// looks at the element that started one.
document.addEventListener("animationstart", (event) => {
  if (!(event.target instanceof Element)) return;

  for (const a of event.target.getAnimations()) {
    if (a.startTime !== 0 && a.effect?.getComputedTiming().iterations === Infinity) a.startTime = 0;
  }
});

// Suppress the webview menu without stopping custom context menu handlers.
document.addEventListener("contextmenu", e => e.preventDefault(), true);
