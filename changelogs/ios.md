# iOS changelog

Earlier releases predate this changelog.

## [Unreleased]

### Changed

- iOS: a running agent session, or one waiting on a question, stays
  followed after you leave it, and shows what it did at once when opened
  again. Returning to the app catches each one up once.

### Fixed

- iOS: preserve composer drafts, focus and scroll during live updates.
- iOS: reject stale approvals after their question or session changes.

### Performance

- iOS: update changed conversation rows independently during streaming.
- iOS: returning to the app shows what changed while it was in the
  background instead of reloading every view, for up to a minute.
