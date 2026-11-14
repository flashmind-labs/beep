### Added

- Desktop: a Plugins page in Settings lists your plugins with their source, permissions and state, and turns each on or off. Install one from a Git repository (`owner/repo`, an optional branch or tag and folder) or from a folder on your computer, update a Git plugin, or remove it after a confirmation.
- Desktop: a plugin shows what it asks for, in plain words, before it installs or updates, and an update shows the permissions it adds. Nothing runs until you allow it.

### Changed

- Desktop: plugins in the app data folder run only after you allow the permissions they ask for, and stop again when an update asks for more. Plugins in the folder named by `BEEP_PLUGINS_DIR` still run without approval, for development.
