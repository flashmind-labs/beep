### Fixed

- iOS: after coming back from the background or moving to another network,
  the app dials every machine again instead of sometimes never
  reconnecting. A machine that does not answer keeps its sessions on the
  list, marked connecting, and is asked again.

### Changed

- iOS: Agents and Home say "Connecting to your machines…" until a machine
  answers, and "No machine is online." when none does, instead of "No
  sessions running."
