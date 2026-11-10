# Android changelog

Earlier releases predate this changelog.

## [Unreleased]

### Changed

- Android: a running agent session, or one waiting on a question, stays
  followed after you leave it, and shows what it did at once when opened
  again. Returning to the app catches each one up once.

### Fixed

- Android: preserve composer drafts, focus and scroll during live updates.
- Android: reject stale approvals after their question or session changes.

### Performance

- Android: update changed conversation rows independently during streaming.
- Android: returning to the app shows what changed while it was in the
  background instead of reloading every view, for up to a minute.
