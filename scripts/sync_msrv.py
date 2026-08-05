#!/usr/bin/env python3
"""Keep the Rust version spelled out in the README in sync with Cargo.toml.

The MSRV lives in exactly one place: `rust-version` in Cargo.toml. Prose
that states it ("Rust 1.93+") is rewritten from there, so the two can
never drift apart. Text that merely mentions a version in passing — a
sample search query such as "rust 1.93 release highlights" — is left
alone: only "Rust <major>.<minor>+" reads as a claim about the MSRV.

Rewrites in place and exits non-zero when it changed something, the
pre-commit convention for a fixing hook.
"""

import re
import sys
from pathlib import Path

MANIFEST = Path("Cargo.toml")
README = Path("README.md")
CLAIM = re.compile(r"Rust \d+\.\d+\+")


def declared_msrv(manifest: str) -> str:
    match = re.search(r'^rust-version\s*=\s*"([^"]+)"', manifest, re.MULTILINE)
    if match is None:
        raise SystemExit("sync_msrv: Cargo.toml declares no rust-version")
    return match.group(1)


def main() -> int:
    version = declared_msrv(MANIFEST.read_text())
    before = README.read_text()
    after = CLAIM.sub(f"Rust {version}+", before)
    if after == before:
        return 0
    README.write_text(after)
    print(f"sync_msrv: rewrote {CLAIM.pattern} in README.md to Rust {version}+")
    return 1


if __name__ == "__main__":
    sys.exit(main())
