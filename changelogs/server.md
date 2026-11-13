# Server changelog

Earlier releases predate this changelog.

## [0.4.2] - 2026-11-12

### Changed

- Server: beep-server only receives problem reports. The assistant, accounts,
  sign-in, push, chats, research, voice, crons and every other server feature
  are removed, along with Postgres. The config keeps `http_listen`, `log_dir`
  and `[reports]`; the server refuses to start while the config still holds
  any other key, so remove the old ones. The Postgres database and the iroh
  port are no longer used and can be dropped.

