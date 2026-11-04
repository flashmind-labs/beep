// Looping CSS animations take their phase from the document's clock instead of
// from when their element appeared, so a redraw never restarts them. Every
// window's entry imports this first.

/// What runs a loop in the CSS (`infinite`). A loop missing here still aligns,
/// but maybe a frame late, through animationstart.
const LOOPING = ".dspin, .dthink.live .lb i";

function align() {
  for (const a of document.getAnimations()) {
    if (a.startTime !== 0 && a.effect?.getComputedTiming().iterations === Infinity) a.startTime = 0;
  }
}

const hasLoop = (n: Node) => n instanceof Element && (n.matches(LOOPING) || n.querySelector(LOOPING) !== null);

// Runs before paint: getAnimations makes the new element's animation exist now,
// so its first frame is already in phase.
new MutationObserver((changes) => {
  if (changes.some((c) => c.type === "attributes" ? hasLoop(c.target) : [...c.addedNodes].some(hasLoop))) align();
}).observe(document, { subtree: true, childList: true, attributes: true, attributeFilter: ["class", "hidden", "style"] });

// animationstart can come after the first paint, so this only catches what the
// observer cannot see, like a stylesheet that just loaded.
document.addEventListener("animationstart", align);
