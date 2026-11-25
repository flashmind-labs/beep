### Fixed

- Linux, macOS and Windows: a slow or stuck disk no longer delays a machine's other status reports.
- Linux, macOS and Windows: one unreachable peer no longer holds up the other file fetches of a synced project.
- Linux, macOS and Windows: a team change or a project import reaches the team board without waiting for a slow desktop.

### Performance

- Linux, macOS and Windows: the device list is read once per change, and only polled while a simulator is listed or a device boots.
- Linux, macOS and Windows: terminal output no longer makes every listener read the terminal list again.
