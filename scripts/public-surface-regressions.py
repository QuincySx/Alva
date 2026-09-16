#!/usr/bin/env python3
"""R70 regression fixtures for the public-surface scanner.

Runs in-repo mutations against `scan_scope` and asserts the snapshot changes
for every breaking contract edit — and stays byte-identical for a
non-breaking whitespace/comment-only control. Exits non-zero on any failure.

Run: `python3 scripts/public-surface-regressions.py`
"""
import runpy
import sys
from pathlib import Path

scanner = runpy.run_path(str(Path(__file__).with_name("public-surface.py")))


def snapshot(source: str) -> str:
    records: list = []
    scanner["scan_scope"](scanner["strip_comments_and_strings"](source), "", True, records)
    return scanner["render"](records)


# (name, before, after, breaking)
CASES = [
    (
        "attributed field type change",
        "pub struct Config { #[cfg(unix)] pub retries: u32 }",
        "pub struct Config { #[cfg(unix)] pub retries: u64 }",
        True,
    ),
    (
        "variant payload change",
        "pub enum E { Data(u32) }",
        "pub enum E { Data(String) }",
        True,
    ),
    (
        "associated type bound change",
        "pub trait T { type Item: Send; }",
        "pub trait T { type Item: Sync; }",
        True,
    ),
    (
        "attributed variant rename",
        "pub enum E { #[cfg(unix)] One, Two }",
        "pub enum E { #[cfg(unix)] Changed, Two }",
        True,
    ),
    (
        "variant after fn-arrow payload",
        "pub enum E { Handler(fn() -> u32), Tail }",
        "pub enum E { Handler(fn() -> u32), Changed }",
        True,
    ),
    (
        "rename behind stacked item attributes",
        "#[derive(Debug)]\n#[repr(u8)]\npub enum E { One }",
        "#[derive(Debug)]\n#[repr(u8)]\npub enum E { Two }",
        True,
    ),
    (
        "whitespace/comment-only edit is not a contract change",
        "pub struct Config {\n    // tuning knob\n    pub retries: u32,\n}",
        "pub struct Config {\n    pub retries: u32,\n} // tuning knob",
        False,
    ),
]


def main() -> int:
    failures = []
    for name, before, after, breaking in CASES:
        left, right = snapshot(before), snapshot(after)
        if breaking and left == right:
            failures.append(f"{name}: breaking edit produced identical snapshots")
        elif not breaking and left != right:
            failures.append(f"{name}: non-breaking edit changed the snapshot\n  {left!r}\n  {right!r}")
        else:
            print(f"ok: {name}")
    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        return 1
    print(f"all {len(CASES)} scanner regression cases passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
