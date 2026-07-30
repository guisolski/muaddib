#!/usr/bin/env sh
set -eu
FAKE_FAIL="expansion" exec "$(cd "$(dirname "$0")" && pwd)/fake-engine.sh" "$@"
