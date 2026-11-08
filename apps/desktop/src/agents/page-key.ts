// Page the conversation when its composer or log has focus.
export function pageDirection(e: Pick<KeyboardEvent, "key" | "altKey" | "ctrlKey" | "metaKey" | "shiftKey" | "isComposing">, inConversation: boolean) {
  if (!inConversation || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || e.isComposing) return 0;
  return e.key === "PageUp" ? -1 : e.key === "PageDown" ? 1 : 0;
}
