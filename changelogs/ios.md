# iOS changelog

Earlier releases predate this changelog.

## [Unreleased]

### Added

- iOS: search the Projects page by project name, folder or remote.

### Changed

- iOS: background commands and queued messages in a session fold into
  one line above the message field, such as "6 running, 4 queued"; tap it
  to list them with Stop, Edit and remove.
- iOS: the Home usage pill shows each harness's icon and the share of its
  most used limit, without bars.
- iOS: the Projects page lists the most recently active projects first
  within each group, after pinned ones.
- iOS: a running agent session, or one waiting on a question, stays
  followed after you leave it, and shows what it did at once when opened
  again. Returning to the app catches each one up once.

### Fixed

- iOS: a session whose turn ended while its subagents still run shows
  as working until they finish, not idle.
- iOS: the message field and its Send and Stop buttons stay fully above
  the keyboard, and a log read to the end stays at the end.
- iOS: the usage popover shows percentages, project and harness tokens,
  models and the total again.
- iOS: the Projects page no longer runs off the side of the screen with
  long names, folders or group headings.
- iOS: preserve composer drafts, focus and scroll during live updates.
- iOS: reject stale approvals after their question or session changes.
- iOS: notifications read, dismissed or opened on any desktop or phone are
  read here too, and opening the app no longer replays ones already read.
- iOS: failed model lookups show Retry, preserve loaded models, and retry sooner.

### Performance

- iOS: update changed conversation rows independently during streaming.
- iOS: returning to the app shows what changed while it was in the
  background instead of reloading every view, for up to a minute.
