import { createStore } from "zustand";

interface ConversationAppearance { scrollbar: boolean; promptMarks: boolean }
type Preference = keyof ConversationAppearance;
const key = (preference: Preference) => `beep.conversation.${preference}`;
const read = (preference: Preference): boolean => {
  try { return localStorage.getItem(key(preference)) !== "false"; } catch { return true; }
};

/// Local appearance preferences shared by settings and every open conversation.
export const conversationAppearance = createStore<ConversationAppearance>(() => ({ scrollbar: read("scrollbar"), promptMarks: read("promptMarks") }));

export function setConversationAppearance(preference: Preference, enabled: boolean) {
  try { localStorage.setItem(key(preference), String(enabled)); } catch { /* Keep the choice for this launch. */ }
  conversationAppearance.setState({ [preference]: enabled });
}

// Other app windows use the same preferences.
window.addEventListener("storage", event => {
  if (event.key === null || event.key === key("scrollbar") || event.key === key("promptMarks")) {
    conversationAppearance.setState({ scrollbar: read("scrollbar"), promptMarks: read("promptMarks") });
  }
});
