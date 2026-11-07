// ---- iPhone live voice (LiveView.swift, LiveSession in the bridge) ----
// Live mode on the phone, as on the Mac: Beep listens, the words show as they are said, a pause
// (or "send") sends, and the reply is read out sentence by sentence before it listens again.
// It opens full screen. In Japan on iOS 26.2+ a press and hold of the side button opens it at once
// (the "activate" assistant intent); elsewhere the chat bar's live button, the Action Button, a
// Control Center control or Shortcuts do. The launch: a soft wash in Beep's colors floods in from
// the side button for 0.25 s, then contracts into a glow hugging the rounded screen edge that keeps
// flowing while live mode is on and brightens with the voice. The orb comes out of the wash and
// settles in the middle. Reduce Motion: the scene fades in, the glow stands still.
(() => {
  const st = document.createElement("style");
  st.textContent = `@property --ga { syntax: '<angle>'; inherits: false; initial-value: 0deg }
@property --ew { syntax: '<length>'; inherits: false; initial-value: 0px }
@property --hole { syntax: '<percentage>'; inherits: false; initial-value: 0% }
/* The edge glow: a ring in Beep's colors, flowing around the rounded screen edge. */
/* Launch: the wash floods in from the side button, then the ring contracts to the edge. */
/* Reduce Motion: the scene fades in; the glow and orb stand still. */
/* The phone's side button, pressed for the launch. */
/* Control Center control. */`;
  document.head.append(st);
})();

IC.wave = '<svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"><path d="M4 10v4M8 7v10M12 4v16M16 7v10M20 10v4"/></svg>';
