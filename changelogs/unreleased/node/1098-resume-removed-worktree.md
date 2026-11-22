### Fixed

- Node: a Claude Code session resumes in the folder it was started in,
  even after it worked in a worktree that was later removed. If that
  folder is gone too, it resumes in the home folder. Resuming no longer
  fails with "Recreate that folder to resume it."
