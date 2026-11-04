#!/usr/bin/env bash
# Runs on a node VM: make-tree.sh <dir> <small files> <1 MB files>
#
# A tree 8 folders deep (two branches per level, 511 folders), the small
# files spread over every folder and the 1 MB random files over the deepest
# ones. Each small file's content is unique.
set -euo pipefail

dir="$1" small="$2" big="$3"
mkdir -p "$dir"
cd "$dir"

folders=(.)
level=(.)
for depth in 1 2 3 4 5 6 7 8; do
  next=()
  for parent in "${level[@]}"; do
    next+=("$parent/d$depth-0" "$parent/d$depth-1")
  done
  level=("${next[@]}")
  folders+=("${next[@]}")
done
mkdir -p "${level[@]}"

count=${#folders[@]}
for ((i = 0; i < small; i++)); do
  printf 'small file %d of %s\n' "$i" "$dir" > "${folders[i % count]}/f$i.txt"
done
for ((i = 0; i < big; i++)); do
  head -c 1048576 /dev/urandom > "${level[i % ${#level[@]}]}/m$i.bin"
done
