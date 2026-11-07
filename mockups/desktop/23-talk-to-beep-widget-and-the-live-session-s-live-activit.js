// ---- Talk to Beep widget and the live session's Live Activity (BeepWidget target) ----
// The widget's button runs the same Talk to Beep intent as the Action Button: the app opens straight
// into live mode with the launch glow (from the middle, as for a control). It has no reply to show,
// since the widget cannot read the app's chats, so both sizes are just the button.
// While a live session runs, a Live Activity shows its state with a small orb in the Dynamic Island
// and on the Lock Screen; a tap goes back to the session, End leaves live mode, and it ends with it.
(() => {
  const st = document.createElement("style");
  st.textContent = ``;
  document.head.append(st);
})();
S.voiceSpeed = "1.25×";
S.toolLabels = S.toolLabels || "run";
S.grants = S.grants || "list";

try { const s = localStorage.getItem("beep-screens"); if (s && D[s]) S.screen = s; } catch (e) {}
const hash = location.hash.slice(1);
if (D[hash]) S.screen = hash;
render();
