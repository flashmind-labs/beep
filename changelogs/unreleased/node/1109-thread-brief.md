### Added

- Node: the assistant has its own tools for a chat's threads:
  `start_thread`, `thread_status`, `send_to_thread`, `stop_thread` and
  `list_threads`. A thread starts on the chat's machine, in a subfolder of
  the chat's folder, on the chat's harness unless another is named.

### Fixed

- Node: the assistant no longer uses its harness's hidden subagents in place
  of threads, so the threads it says it started show under the chat.
