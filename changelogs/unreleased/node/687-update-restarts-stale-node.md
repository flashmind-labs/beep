### Fixed

- Node: `beep-node update` and `update --now` restart the node on the installed version when it is already the newest release but the running node is older, instead of reporting it as up to date. `--now` restarts at once; otherwise the restart waits until agent sessions finish their turns. `update --check` reports that the node still runs the older version.
