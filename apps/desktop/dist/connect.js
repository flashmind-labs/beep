// Why the command center could not load Home, read from the error the app
// gave, so a problem waiting will not fix shows as itself and not as
// "Still connecting".

/// What to show for `error`, or null when trying again may help, such as
/// a slow or lost connection or a server error. `view` is the pane page
/// that fixes it, when there is one.
export function connectProblem(error) {
  const e = String(error ?? "");
  if (e === "not linked to a phone yet") {
    return {
      kind: "unlinked",
      title: "This Mac is not linked yet",
      text: "Link it to your phone to see your chats, machines and approvals here.",
      action: "Link to your phone",
      view: "link",
    };
  }
  if (e === "this computer was removed from the account") {
    return {
      kind: "revoked",
      title: "This Mac was removed",
      text: "Your phone took this computer off the account. To use Beep here again, start over and link it from the phone.",
      action: "Start over",
      view: "revoked",
    };
  }
  // The server answered and said no; asking again gets the same answer.
  const refused = /^(?:bad request|conflict): (.+)$/s.exec(e);
  if (refused || e === "not found") {
    return {
      kind: "refused",
      title: "The server turned this Mac away",
      text: refused ? refused[1] : "It did not find this Mac's account.",
      action: null,
      view: null,
    };
  }
  return null;
}
