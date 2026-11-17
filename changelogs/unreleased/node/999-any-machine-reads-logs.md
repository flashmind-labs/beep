### Changed

- Node: an agent on a node can now read every machine's status, logs and
  metrics and include them in a problem report, as an agent on a desktop
  can. Before, a node refused these requests from another node, so the
  beep_logs, beep_status, beep_metrics and report tools only covered that
  node. Machines still on an older node release keep refusing until they
  update.
