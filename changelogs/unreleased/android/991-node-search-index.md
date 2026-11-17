### Added

- Android: the search asks your online machines directly, and it finds
  agent replies too.

### Changed

- Android: sessions on offline machines are not found by their text.
- Android: search by text only covers sessions that ran inside a project
  imported on that machine. Sessions in other folders are still listed,
  but they are not found by what was said in them.

### Performance

- Android: the phone no longer downloads transcripts to build a search
  index, which saves data, battery and storage.
