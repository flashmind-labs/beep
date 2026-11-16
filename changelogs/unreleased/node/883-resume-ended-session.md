### Fixed

- Node: a message sent to a session that has ended opens the session
  again and goes to it, instead of failing with "session is no longer on
  the machine". This covers messages held while offline and sent later.
