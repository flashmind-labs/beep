// Whether the chat runs on this Mac: no server, and an OpenRouter key saved. Then the window shows
// the Chats group and the chat pages it shows with a subscription, and Rust answers them from the
// chats kept here (`chat/sessions.rs`, the `local` branch). Rust decides; this only asks.
import { S } from "../bar/core.ts";
import { invoke } from "../ui/tauri.ts";

/// Asks Rust and keeps the answer in `S.local`. True when it changed.
export async function refreshLocal() {
  const was = S.local;
  S.local = await invoke<boolean>("assistant_local").catch(e => { console.warn("asking if chat is local:", e); return false; });
  return S.local !== was;
}

/// Whether chats show: with a subscription, or kept on this Mac.
export const chatsOn = (subscribed: boolean) => subscribed || S.local;
