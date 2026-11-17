#!/usr/bin/env bash
# Tauri forces --color always after the runner arguments, overriding Cargo's environment.
set -euo pipefail

args=()
while (($#)); do
    case "$1" in
        --color)
            shift 2
            ;;
        --color=*)
            shift
            ;;
        --)
            args+=("$@")
            break
            ;;
        *)
            args+=("$1")
            shift
            ;;
    esac
done

exec cargo --color never "${args[@]}"
