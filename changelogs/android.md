# Android changelog

Earlier releases predate this changelog.

## [Unreleased]

### Added

- Android: search the Projects page by project name, folder or remote.

### Changed

- Android: the Home usage pill shows each harness's icon and the share of
  its most used limit, without bars.
- Android: the Projects page lists the most recently active projects first
  within each group, after pinned ones.
- Android: a running agent session, or one waiting on a question, stays
  followed after you leave it, and shows what it did at once when opened
  again. Returning to the app catches each one up once.

### Fixed

- Android: the usage popover shows percentages, project and harness tokens,
  models and the total again.
- Android: the Projects page no longer runs off the side of the screen with
  long names, folders or group headings.
- Android: preserve composer drafts, focus and scroll during live updates.
- Android: reject stale approvals after their question or session changes.

### Performance

- Android: update changed conversation rows independently during streaming.
- Android: returning to the app shows what changed while it was in the
  background instead of reloading every view, for up to a minute.
