### Fixed

- Node: a session past its compaction limit compacts as soon as its turn
  ends, so the next message no longer waits behind a compaction that starts
  only when the message is sent.
