#!/usr/bin/env sh
set -eu
dir="$(cd "$(dirname "$0")" && pwd)"
marker=""
for arg in "$@"; do
  case "$arg" in
    *"MUADDIB:EXPAND"*) marker="expansion" ;;
    *"MUADDIB:SUBSEARCH"*) marker="subsearch" ;;
    *"MUADDIB:SYNTH"*) marker="synthesis" ;;
    *"MUADDIB:FAST"*) marker="fast" ;;
  esac
done
if [ -z "$marker" ]; then
  echo "no muaddib task marker found in arguments" >&2
  exit 64
fi
if [ "${FAKE_FAIL:-}" = "$marker" ]; then
  echo "forced failure for $marker" >&2
  exit 65
fi
cat "$dir/${marker}_response.json"
