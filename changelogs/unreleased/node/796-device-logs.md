### Added

- Node: agents read a device's newest log lines with the `device` tool's
  `logs` action: logcat for Android phones and emulators, the last five
  minutes of an iOS simulator's log, with an optional filter and line count.
- Node: the `device` tool's `ui` action lists what the screen shows, each
  element's label, id and where to tap it, for Android devices and iOS
  simulators.
- Node: the `device` tool installs an APK or `.app` already on the device's
  machine (`install`), starts an app or opens a link (`launch`), and stops an
  Android app and clears its data (`reset`).
