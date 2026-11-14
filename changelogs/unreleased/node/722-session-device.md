### Added

- Node: a session keeps the emulator or simulator its agent last used with the `device` tool, also across a node restart.

### Changed

- Node: agents are told how to install and start an app on an emulator or
  simulator, to reuse a device that is already on, and not to shut down one
  the user may be using.
