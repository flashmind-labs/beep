### Performance

- Desktop: Git sync between machines sends only the commits the other machine
  is missing, instead of the whole history for each new branch. A repository
  that grew to tens of gigabytes from repeated copies stays its normal size.
- Desktop: only the default branch and the branch checked out on each machine
  sync. Agent worktree branches stay on the machine that made them. A branch
  that both machines have fast-forwards while it is checked out on the
  machine that made the commits.

### Fixed

- Desktop: an interrupted Git transfer no longer leaves half-written pack files
  in `.git`. A stalled transfer gives up after 30 seconds of silence instead
  of waiting five minutes. Git packs its objects together once many have
  arrived.
