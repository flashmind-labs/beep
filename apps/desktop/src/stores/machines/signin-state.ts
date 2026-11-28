import { isRecord } from "../../ui/plugin-rows.ts";

// How a machine's coding agent signs in from this Mac, as pure functions.
// signin.ts draws the sheet.

/// How a sign-in stands on the machine (SignInView).
export interface SignInView {
  state: "idle" | "starting" | "waiting" | "signed_in" | "failed";
  url: string | null;
  code: string | null;
  asks_code: boolean;
  reason: string | null;
}

/// The agents a machine signs in for this Mac: it runs their own sign-in.
const REMOTE = new Set(["claude", "codex", "opencode", "agy"]);

/// What a Sign-ins row offers for `tool` when it is not signed in on
/// `machine`: Sign in, or how to sign in there. Nothing for this Mac, where
/// a terminal is at hand, or for a machine offline.
export function signInOffer(tool: string, machine: string, online: boolean, thisMac: boolean): { button: true } | { how: string } | null {
  if (thisMac) return null;
  if (REMOTE.has(tool)) return online ? { button: true } : null;
  if (tool === "pi") return { how: `Run pi on ${machine} and use /login` };
  if (tool === "gh") return { how: `Run gh auth login on ${machine}, or add a GitHub token in Credentials` };
  return null;
}

/// Whether the sheet keeps asking how the sign-in stands.
export const stillRunning = (view: SignInView) => view.state === "starting" || view.state === "waiting";

const text = (value: unknown) => typeof value === "string" ? value : null;

/// The sign-in a `sign-in` update carries (the node's `SignIn` state), as the sheet draws it; null
/// when the update has none, as after a refused snapshot.
export function readSignIn(data: unknown): SignInView | null {
  if (!isRecord(data) || !isRecord(data.SignIn)) return null;

  const [variant, body] = Object.entries(data.SignIn)[0] ?? [];
  const fields = isRecord(body) ? body : {};
  const view: SignInView = { state: "idle", url: null, code: null, asks_code: false, reason: null };

  if (variant === "Starting") view.state = "starting";
  else if (variant === "Waiting") Object.assign(view, { state: "waiting", url: text(fields.url), code: text(fields.code), asks_code: fields.asks_code === true });
  else if (variant === "SignedIn") view.state = "signed_in";
  else if (variant === "Failed") Object.assign(view, { state: "failed", reason: text(fields.reason) });

  return view;
}
