### Added

- iOS: the search asks your online machines directly, and it finds
  agent replies too.

### Changed

- iOS: sessions on offline machines are not found by their text.
- iOS: search by text only covers sessions that ran inside a project
  imported on that machine. Sessions in other folders are still listed,
  but they are not found by what was said in them.

### Performance

- iOS: the phone no longer downloads transcripts to build a search
  index, which saves data, battery and storage.
