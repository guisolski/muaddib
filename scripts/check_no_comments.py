#!/usr/bin/env python3
"""Reject Rust sources containing comments.

Project convention: no comments of any kind (//, ///, //!, /* */).
Intent belongs in named functions and in /docs.
String, char, and raw-string literals are skipped so that e.g. URLs
containing // never trigger a false positive.
"""

import sys


def find_comment_lines(source: str) -> list[int]:
    lines: list[int] = []
    line = 1
    i = 0
    n = len(source)
    while i < n:
        ch = source[i]
        if ch == "\n":
            line += 1
            i += 1
            continue
        if ch == "/" and i + 1 < n and source[i + 1] in "/*":
            lines.append(line)
            if source[i + 1] == "/":
                while i < n and source[i] != "\n":
                    i += 1
            else:
                depth = 1
                i += 2
                while i < n and depth > 0:
                    if source.startswith("/*", i):
                        depth += 1
                        i += 2
                    elif source.startswith("*/", i):
                        depth -= 1
                        i += 2
                    else:
                        if source[i] == "\n":
                            line += 1
                        i += 1
            continue
        if ch == '"':
            i, line = skip_string(source, i, line)
            continue
        if ch == "r" and is_raw_string_start(source, i):
            i, line = skip_raw_string(source, i, line)
            continue
        if ch == "'":
            i = skip_char_or_lifetime(source, i)
            continue
        i += 1
    return lines


def skip_string(source: str, i: int, line: int) -> tuple[int, int]:
    i += 1
    n = len(source)
    while i < n:
        if source[i] == "\\":
            i += 2
            continue
        if source[i] == "\n":
            line += 1
        if source[i] == '"':
            return i + 1, line
        i += 1
    return i, line


def is_raw_string_start(source: str, i: int) -> bool:
    j = i + 1
    if j < len(source) and source[j] == "b":
        j += 1
    while j < len(source) and source[j] == "#":
        j += 1
    return j < len(source) and source[j] == '"'


def skip_raw_string(source: str, i: int, line: int) -> tuple[int, int]:
    j = i + 1
    if source[j] == "b":
        j += 1
    hashes = 0
    while source[j] == "#":
        hashes += 1
        j += 1
    j += 1
    closer = '"' + "#" * hashes
    n = len(source)
    while j < n:
        if source[j] == "\n":
            line += 1
        if source.startswith(closer, j):
            return j + len(closer), line
        j += 1
    return j, line


def skip_char_or_lifetime(source: str, i: int) -> int:
    n = len(source)
    if i + 2 < n and source[i + 1] == "\\":
        j = i + 2
        while j < n and source[j] != "'":
            j += 1
        return j + 1
    if i + 2 < n and source[i + 2] == "'":
        return i + 3
    return i + 1


def main(paths: list[str]) -> int:
    failed = False
    for path in paths:
        with open(path, encoding="utf-8") as handle:
            offending = find_comment_lines(handle.read())
        for line in offending:
            print(f"{path}:{line}: comment found; extract a named function instead")
            failed = True
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
