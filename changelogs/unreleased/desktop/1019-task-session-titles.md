### Added

- Desktop: an issue's page shows the agent session working on it. While
  it runs, the button reads "Working" and the agent's name and opens its
  chat. After it finishes, the button reads "Open session". Both replace
  "Work on it".
- Desktop: a failed Actions run shows its agent session the same way. The
  button reads "Fixing" and the agent's name while it runs, then "Open
  session" when it finishes. Both replace "Fix this".
- Desktop: sessions started from "Work on it" are named "Issue #N", and
  sessions started from "Fix this" are named "Fix run #N
  (workflow)", as PR reviews are named "Review #N". The names cannot be
  changed, so a session can be found from any machine. Sessions started
  on an issue before this update get their name when the issue is opened.
