### Fixed

- Node: usage and cost views counted all of a flsh session's spend in
  the hour of its last message, so a resumed session showed its whole
  cost as today's. Each flsh turn's spend now counts in the hour it
  happened, for sessions written by a flsh version that logs turns.
  Older sessions keep their earlier spend in one lump.
