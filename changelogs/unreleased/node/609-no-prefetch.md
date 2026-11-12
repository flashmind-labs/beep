### Changed

- Node: sessions from other machines are no longer copied in the
  background. A session is copied when you continue it on another machine,
  and both machines must be online. Copies left from earlier versions are
  deleted on start. Versions of a session that lost to another machine's
  are kept on disk.
