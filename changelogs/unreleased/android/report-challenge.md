### Added

- Android: a report in a session's chat has a Challenge button. It asks which
  harness, then which model, to challenge with, and starts a fresh agent on it
  that tries to prove the report's claims wrong in a throwaway copy of the work.
  The header shows it running, with Open to see its session and Stop to stop it
  and remove its session and copy, then how many findings it left.
- Android: a challenger that asks you something shows "Challenger asks you" in
  place of the spinner. One that stops without its report, fails to start, or
  whose session is deleted shows "Challenge failed", with the reason on hover,
  Open, and Again to pick a model and challenge once more.
- Android: its findings arrive as review drafts under the claims they dispute,
  headed Challenge with a CONFIRMED (it ran something) or PLAUSIBLE (it read the
  code) badge, their words, and their evidence on a line of its own, with code
  drawn as code. They go with your next message, first, and the agent is asked
  to fix each or show why it is wrong.
- Android: a saved review comment's card has a × to remove it.
- Android: the review line over the message field has Send now, which sends the
  review on its own, without a message.

### Fixed

- Android: the words a review comment is about stay marked, underlined, after
  the comment is saved.
- Android: the model picker shows a model's whole name, wrapping a long one,
  with the provider and id that tell apart models of the same name.
