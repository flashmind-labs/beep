### Changed

- Desktop: adding a project to a machine, and Change folder, now picks the
  folder in a search palette of that machine's folders. The palette shows
  the path as steps you can go back through. A field filters the folders
  shown, or takes a typed path. Git checkouts have a branch icon. One row
  says exactly where the project will go. Up and Down move, Enter takes the
  highlighted row, Cmd+Enter (Ctrl+Enter on Windows) adds the project, and
  Backspace goes up one folder.

### Fixed

- Desktop: a project can be added to a folder that already holds its files
  without `.git`, for example files copied there by another file sync. The
  Git history arrives and the files stay as they are. Adding no longer
  stops with "has other files" or makes a nested `project/project` folder.
- Desktop: in the Import and Add a folder sheets, a long path in the folder
  browser's note no longer runs under its buttons.
