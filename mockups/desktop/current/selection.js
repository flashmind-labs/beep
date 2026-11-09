// Capture the last selection before earlier previews perform their first draw.
const MOCKUP_SELECTION = (() => {
  try { return localStorage.getItem("beep-screens"); } catch { return null; }
})();
