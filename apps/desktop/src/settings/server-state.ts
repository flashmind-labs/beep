// What Settings' Beep server section offers and shows, and what says a Mac is
// signed out in the bar and the window. Pure; settings.ts draws it.

import { SUBSCRIPTION_NOTE } from "./subscription.ts";

/// Whether the server answered the last list of its devices (`list_devices`).
export type Reach = "none" | "answered" | "unreachable";

/// Where this Mac stands with the Beep server.
export type ServerKind = "signed-in" | "unreachable" | "signed-out" | "no-account";

/// A button of the section.
export type ServerAction = "sign-out" | "sign-in" | "retry";

export interface ServerView {
  title: string;
  note: string;
  /// The dot beside the title: on, off or failing.
  dot: "ok" | "off" | "bad";
  actions: ServerAction[];
  /// Whether the account's devices are listed under it.
  devices: boolean;
}

/// Signed out wins over everything: the Mac makes no calls, so nothing was reached. Without
/// an account (`link` is not linked) there is nothing to sign out of. `reach` is null while the
/// devices load, which reads as signed in.
export function serverKind(link: "unlinked" | "linked" | "revoked", signedOut: boolean, reach: Reach | null): ServerKind {
  if (signedOut) return "signed-out";
  if (link !== "linked") return "no-account";
  return reach === "unreachable" ? "unreachable" : "signed-in";
}

/// What the section says and offers in each state.
export function serverView(kind: ServerKind): ServerView {
  switch (kind) {
    case "signed-in":
      return { title: "Signed in", note: "Chats, memories and routines sync with the server.", dot: "ok", actions: ["sign-out"], devices: true };
    case "unreachable":
      return { title: "Not reachable", note: "Beep cannot reach the server. Try again, or sign out.", dot: "bad", actions: ["retry", "sign-out"], devices: false };
    case "signed-out":
      return { title: "Signed out", note: "The assistant and voice are off. Machines, agents and projects keep working.", dot: "off", actions: ["sign-in"], devices: false };
    case "no-account":
      return { title: "No account", note: SUBSCRIPTION_NOTE, dot: "off", actions: ["sign-in"], devices: false };
  }
}

export const ACTION_LABEL: Record<ServerAction, string> = {
  "sign-out": "Sign out…",
  "sign-in": "Sign in",
  retry: "Try again",
};

/// The question before signing out: what stops, in a line.
export const SIGN_OUT_ASK = {
  title: "Sign out of Beep server?",
  note: "This Mac stops using the server, so the assistant and voice go off. Signing in brings it all back.",
  confirm: "Sign out",
  cancel: "Cancel",
};

/// The state beside the bar's hints, as a dot and a word: none while it is nothing to say.
export function linkChip(server: boolean, link: "unlinked" | "linked" | "revoked", signedOut: boolean): ["off" | "bad", string] | null {
  if (!server) return null;
  if (link === "revoked") return ["bad", "Removed"];
  if (link !== "unlinked") return null;
  return ["off", signedOut ? "Signed out" : "No subscription"];
}

/// The window sidebar's footer while signed out: the state and the button that leads to Sign in.
/// Null when signed in or without an account, which keep the machine count.
export function signedOutFooter(server: boolean, signedOut: boolean): { text: string; action: string } | null {
  return server && signedOut ? { text: "Signed out", action: "Sign in" } : null;
}
