### Changed

- Desktop: adding a project to a machine where the chosen folder sits
  inside a Syncthing shared folder no longer fails. Beep adds the folder
  to Syncthing's ignore list on that machine and syncs it itself. The
  folder picker says so before you add. Projects set to use Syncthing are
  still refused.
