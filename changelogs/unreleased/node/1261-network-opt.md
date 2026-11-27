### Performance

- Node: Syncs projects only when mesh peers are connected, sharing single connections across projects and backing off reconnect attempts to offline peers.
- Node: Compresses large network messages with zstd when supported by peers to reduce bandwidth usage.
- Node: Sends incremental updates for session queries and live transcripts instead of retransmitting full snapshots on every change, with slower update cadence for background sessions.
