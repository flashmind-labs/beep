### Changed

- Desktop: with Project sync on, projects inside a Syncthing share leave
  Syncthing on every machine, including projects imported before and
  after, so only Beep sync writes them. The "Always use Beep sync over
  Syncthing" setting is gone. A project's menu can still hand it back to
  Syncthing.

### Fixed

- Desktop: adding a project that uses Beep sync to a machine where the
  folder is inside a Syncthing share adds the Syncthing ignore rule there
  instead of refusing the folder.
