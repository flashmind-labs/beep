### Performance

- Sessions started outside Beep are read from disk once per change instead of twice, so lists and search catch up sooner.

### Fixed

- Opening a file in a folder known only from the agents' logs no longer waits behind a session listing.
