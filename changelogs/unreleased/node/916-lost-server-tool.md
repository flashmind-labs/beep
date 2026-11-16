### Fixed

- Node: a Claude Code turn that finished normally no longer ends with
  "The turn did not finish" when the same step asked for the advisor
  next to a local command. The advisor request never ran, so that turn
  now ends normally.
