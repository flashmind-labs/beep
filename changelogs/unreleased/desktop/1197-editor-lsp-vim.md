### Added

- Desktop: Cmd+click (Ctrl+click on Linux and Windows) in the file viewer and
  editor, as well as `gd` in Vim mode, jumps to a symbol's definition using the
  machine's installed language server, offering a picker when multiple
  definitions match.

### Changed

- Desktop: Editor Vim mode preference now syncs across desktops via mesh state
  in addition to local storage, ensuring the choice persists across restarts and
  machines.
