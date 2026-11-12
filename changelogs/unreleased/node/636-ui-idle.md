### Performance

- Node: runs git less often while agents work: a git command no longer makes
  it rewatch every folder and read them all again, file sync reads the
  checked-out commit once per change, and a folder's file list waits longer
  between reads while it stays the same.
