### Changed

- Server: beep-server only receives problem reports. The assistant, accounts,
  sign-in, push, chats, research, voice, crons and every other server feature
  are removed, along with Postgres. The config keeps `http_listen`, `log_dir`
  and `[reports]`.
