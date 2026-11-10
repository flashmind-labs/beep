// Looping CSS animations take their phase from the document's clock instead of
// from when their element appeared, so a redraw never restarts them. Every
// window's entry imports this first. Only spinners for the user's own waits and
// the open transcript still loop, so this looks at the element that started one.
document.addEventListener("animationstart", (event) => {
  if (!(event.target instanceof Element)) return;

  for (const a of event.target.getAnimations()) {
    if (a.startTime !== 0 && a.effect?.getComputedTiming().iterations === Infinity) a.startTime = 0;
  }
});
