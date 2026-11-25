### Added

- Node: on Linux and macOS, sessions give their agent a Docker socket of
  their own, so the containers it starts with `docker run` or
  `docker compose up` show in the session with their published ports.
  Closing the session stops them, and nothing is removed. It applies to
  sessions started after the setting changes.
