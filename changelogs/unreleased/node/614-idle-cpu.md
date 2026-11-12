### Performance

- Node: an idle node starts far fewer background processes. The machine's
  tool and sign-in check runs once for every status connection and for
  `list_machines`, path checks no longer run `git` for each removed
  worktree, and idle session process and open-file checks run less often.
