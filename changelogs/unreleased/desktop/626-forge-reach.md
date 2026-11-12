### Fixed

- Desktop: pull requests and issues on a git host this Mac cannot reach are read through another
  machine with repositories on that host, and "Could not reach" shows only when no machine can.
- Desktop: a git host no machine reaches is tried again every few minutes instead of on every
  refresh, and its failures no longer fill the log.
