### Performance

- Desktop: a synced Git project checks only the branches that moved
  when another machine announces its branches, instead of running Git
  for every branch.
- Desktop: a file or Git view whose read keeps failing retries less
  often instead of up to four times a second.
- Desktop: checking whether a path lies in a session's worktrees runs
  Git again only after worktrees are added or removed.
- Desktop: idle agent sessions share one process list read about once
  a minute instead of each reading its own.
