#!/usr/bin/env bash
# fd-sample.sh <out> : every 2 s, one line for the node process:
# <epoch> <open fds> <inotify instances> <inotify watches>
out="$1"
while :; do
  p="$(cat ~/beep/node.pid 2> /dev/null)"
  if [[ -n "$p" && -d "/proc/$p/fd" ]]; then
    fds=0 instances=0 watches=0
    for f in "/proc/$p/fd/"*; do
      fds=$((fds + 1))
      if [[ "$(readlink "$f" 2> /dev/null)" == anon_inode:inotify ]]; then
        instances=$((instances + 1))
        watches=$((watches + $(grep -c '^inotify wd:' "/proc/$p/fdinfo/${f##*/}" 2> /dev/null)))
      fi
    done
    echo "$(date +%s) $fds $instances $watches" >> "$out"
  fi
  sleep 2
done
