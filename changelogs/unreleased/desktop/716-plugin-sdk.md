### Added

- Desktop: `beep.log` and `console.log`, `console.warn` and `console.error`
  lines from a plugin go to the Beep log file, tagged with the plugin's id.
- Desktop: a plugin can keep small data between restarts with `beep.store`.
  Removing the plugin deletes its data.
- Desktop: a plugin can be split over several files with relative imports.
- Desktop: a plugin names the plugin API version it needs in its manifest,
  `"api": 1`. A plugin that needs a newer Beep shows why in Settings,
  Plugins instead of running.
