#!/usr/bin/env bash
set -euo pipefail

require_cargo_mutants() {
    if ! command -v cargo-mutants >/dev/null 2>&1; then
        echo "cargo-mutants is not installed. Run: make hooks" >&2
        exit 1
    fi
}

base_ref() {
    git rev-parse --verify --quiet origin/main \
        || git rev-parse --verify --quiet main \
        || true
}

diff_start() {
    local base
    base="$(base_ref)"
    git merge-base "${base:-HEAD}" HEAD 2>/dev/null \
        || git rev-parse --verify --quiet HEAD~1 \
        || git rev-parse HEAD
}

require_cargo_mutants

diff_file="$(mktemp)"
trap 'rm -f "$diff_file"' EXIT

git diff "$(diff_start)" -- src > "$diff_file"

if [ ! -s "$diff_file" ]; then
    echo "mutants: no changes under src/ to mutate"
    exit 0
fi

exec cargo mutants --no-shuffle --minimum-test-timeout 60 --in-diff "$diff_file"
