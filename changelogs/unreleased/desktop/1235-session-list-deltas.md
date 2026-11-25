### Performance

- Desktop: follows a node's session list by the sessions that changed, so Stop, queued messages and session views on a remote machine no longer wait behind the list.
- Desktop: the outbox logs each try at a queued message (check, send, upload, resume, configure) with its result and time, so a retry loop shows in the log.
