### Fixed

- A project page no longer shows a machine's sync state from an old answer after the machine reconnects.

### Performance

- A project page checks Syncthing on all machines at once, so a slow or unreachable machine delays its Syncthing row by 3 seconds at most instead of 3 seconds each.
