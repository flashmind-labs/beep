### Changed

- Desktop: lean builds download remote media in bounded chunks. Resumable media caching remains available with `blob-cache` or `full`.
- Desktop: plugin interfaces and themes remain available in lean builds. The plugin tool API requires `plugin-tools` or `full`; authors can check `beep.capabilities.tools` before registering tools. The Session Notes interface continues to work.

### Fixed

- Desktop: media viewers reject incomplete or changing remote downloads instead of displaying partial files.
