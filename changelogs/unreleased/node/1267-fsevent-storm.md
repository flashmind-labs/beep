### Performance

- Stop short-lived usage streams and timestamp-only plan updates from repeatedly spawning agent CLIs on every background spend refresh on macOS and Linux nodes.
- Filter noisy non-session files (logs, SQLite `-shm` sidecars, empty `-wal` files, and hidden skill directories) and debounce `agent` change notifications so idle nodes avoid filesystem watcher storms and redundant session snapshot reads.
