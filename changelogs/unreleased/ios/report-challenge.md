### Added

- iOS: a report in a session's chat has a Challenge button. It asks which
  harness, then which model, to challenge with, and starts a fresh agent on it
  that tries to prove the report's claims wrong in a throwaway copy of the work.
  The header shows it running, with Open to see its session and Stop to stop it
  and remove its session and copy, then how many findings it left, which goes to
  them. A finished challenger's session and copy are removed; a failed one's
  session stays to show why until the next challenge.
- iOS: a challenger that asks you something shows "Challenger asks you" in place
  of the spinner. One that stops without its report, fails to start, or whose
  session is deleted shows "Challenge failed", with the reason under the header,
  Open, and Again to pick a model and challenge once more.
- iOS: its findings arrive as review drafts under the claims they dispute,
  headed Challenge with a CONFIRMED (it ran something) or PLAUSIBLE (it read the
  code) badge, their words, and their evidence on a line of its own, with code
  drawn as code. They go with your next message, first, and the agent is asked
  to fix each or show why it is wrong.
- iOS: a saved review comment's card has a × to remove it.
- iOS: the review line over the message field has Send now, which sends the
  review on its own, without a message.
- iOS: comments and findings sent in a review stay on the claims they are about,
  one line each marked "sent", their words underlined thinly; a click shows the
  whole comment, and "answered ↓" goes to the reply that answered it.
- iOS: a finding whose quoted words aren't in the report, because the challenger
  paraphrased them, goes under the claim it names, with that claim's first
  sentence marked, instead of at the end of the report.
- iOS: when a challenge finishes, the claims it checked and could not break show
  a green HELD tag on the report, and the header says how many are held, such as
  "2 held"; hovering lists them.

### Fixed

- iOS: the words a review comment is about stay marked, underlined, after the
  comment is saved.
- iOS: the model picker shows a model's whole name, wrapping a long one, with
  the provider and id that tell apart models of the same name.
