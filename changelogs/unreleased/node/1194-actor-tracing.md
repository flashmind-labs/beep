### Added

- Node: metrics for how long each internal worker's messages wait and take
  to handle, and how long its background jobs take (process scans, git
  reads, sync changes). `beep_metrics` shows them as `beep_actor_wait_seconds`,
  `beep_actor_handle_seconds` and `beep_actor_job_seconds`.
- Node: a log line when one of those messages waits or runs long, naming the
  worker and the kind of message.

### Changed

- Node: the log of a failed request now says whether the node refused it,
  with the reason, or could not send the answer. A stream that ends is no
  longer logged as a slow or cancelled request.
