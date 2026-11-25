### Fixed
- Linux, macOS and Windows nodes: a parent agent waiting on an agent on another machine now hears when it finishes even if the first message was lost on a dropped link.
- Linux, macOS and Windows nodes: a deleted conversation no longer comes back in the list for up to a minute.
- Linux, macOS and Windows nodes: background agents that finish are removed at once, not up to a minute later.

### Performance
- Linux, macOS and Windows nodes: several open usage views share one read of the agents' logs instead of one each.
