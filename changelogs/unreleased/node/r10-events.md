### Changed

- Node: beep-node notices sooner when the parent session stops keeping one of its agents. It follows the parent's agent list as the parent sends it, which means less network traffic.
- Node: the machine that watches CI runs is picked from live reachability updates, and picked again when it stops reporting.
