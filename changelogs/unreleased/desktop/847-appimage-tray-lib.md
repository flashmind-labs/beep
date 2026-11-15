### Fixed

- Desktop: on Linux, the AppImage no longer crashes at start. It now
  includes the tray icon library (ayatana-appindicator) instead of
  relying on the system copy. The .deb package depends on that library.
